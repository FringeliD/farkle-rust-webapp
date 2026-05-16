use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use rand::Rng;
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fs,
    net::SocketAddr,
    path::Path,
    sync::{Arc, Mutex},
};
use tower_http::services::{ServeDir, ServeFile};

const SAVE_PATH: &str = "data/save.json";
const TARGET_SCORE: u32 = 10_000;
const LOG_LIMIT: usize = 40;

#[derive(Clone)]
struct AppState {
    game: Arc<Mutex<GameState>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
enum DieStatus {
    Empty,
    Available,
    Locked,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Die {
    id: usize,
    value: Option<u8>,
    status: DieStatus,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct GameState {
    total_score: u32,
    turn_score: u32,
    target_score: u32,
    dice: Vec<Die>,
    selected_dice: Vec<usize>,
    locked_dice: Vec<usize>,
    can_roll: bool,
    can_bank: bool,
    is_bust: bool,
    is_hot_dice: bool,
    is_won: bool,
    message: String,
    log: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct SelectRequest {
    dice_ids: Vec<usize>,
}

#[tokio::main]
async fn main() {
    ensure_data_dir();
    let game = load_game().unwrap_or_else(new_game_state);
    let state = AppState {
        game: Arc::new(Mutex::new(game)),
    };

    let app = Router::new()
        .route("/api/state", get(api_state))
        .route("/api/new", post(api_new))
        .route("/api/roll", post(api_roll))
        .route("/api/select", post(api_select))
        .route("/api/bank", post(api_bank))
        .route("/api/reset", post(api_reset))
        .fallback_service(
            ServeDir::new("public").not_found_service(ServeFile::new("public/index.html")),
        )
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], 8080));
    println!("Farkle is running at http://127.0.0.1:8080");

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("failed to bind to 127.0.0.1:8080");
    axum::serve(listener, app)
        .await
        .expect("server stopped unexpectedly");
}

async fn api_state(State(state): State<AppState>) -> Json<GameState> {
    Json(state.game.lock().expect("game lock poisoned").clone())
}

async fn api_new(State(state): State<AppState>) -> Json<GameState> {
    let mut game = new_game_state();
    push_log(&mut game, "New game started.");
    save_game(&game);
    *state.game.lock().expect("game lock poisoned") = game.clone();
    Json(game)
}

async fn api_reset(State(state): State<AppState>) -> Json<GameState> {
    let mut game = new_game_state();
    game.message = "Save reset. New game ready.".to_string();
    push_log(&mut game, "Save file reset.");
    save_game(&game);
    *state.game.lock().expect("game lock poisoned") = game.clone();
    Json(game)
}

async fn api_roll(State(state): State<AppState>) -> impl IntoResponse {
    let mut guard = state.game.lock().expect("game lock poisoned");
    let game = &mut *guard;

    if game.is_won {
        game.message = "You already won. Start a new game to play again.".to_string();
        save_game(game);
        return (StatusCode::BAD_REQUEST, Json(game.clone()));
    }

    if !game.can_roll {
        game.message = "Select at least one valid scoring die before rolling again.".to_string();
        save_game(game);
        return (StatusCode::BAD_REQUEST, Json(game.clone()));
    }

    if game.is_hot_dice || locked_count(game) == 6 || game.is_bust {
        clear_turn_dice(game);
    }

    let roll_ids: Vec<usize> = game
        .dice
        .iter()
        .filter(|die| die.status != DieStatus::Locked)
        .map(|die| die.id)
        .collect();
    let dice_to_roll = if roll_ids.is_empty() {
        (0..6).collect()
    } else {
        roll_ids
    };

    let mut rng = rand::thread_rng();
    let mut rolled_values = Vec::new();
    for id in dice_to_roll {
        if let Some(die) = game.dice.iter_mut().find(|die| die.id == id) {
            let value = rng.gen_range(1..=6);
            die.value = Some(value);
            die.status = DieStatus::Available;
            rolled_values.push(value);
        }
    }

    game.selected_dice.clear();
    game.is_hot_dice = false;
    game.can_roll = false;
    game.can_bank = false;

    if !has_any_score(&rolled_values) {
        game.is_bust = true;
        game.turn_score = 0;
        game.can_roll = true;
        game.can_bank = false;
        game.message = "Farkle! No scoring dice. Your turn score is lost.".to_string();
        push_log(game, &format!("Rolled {:?}: Farkle.", rolled_values));
    } else {
        game.is_bust = false;
        game.message = "Roll complete. Tap scoring dice to keep them.".to_string();
        push_log(game, &format!("Rolled {:?}.", rolled_values));
    }

    save_game(game);
    (StatusCode::OK, Json(game.clone()))
}

async fn api_select(
    State(state): State<AppState>,
    Json(payload): Json<SelectRequest>,
) -> impl IntoResponse {
    let mut guard = state.game.lock().expect("game lock poisoned");
    let game = &mut *guard;

    if game.is_won {
        game.message = "You already won. Start a new game to play again.".to_string();
        save_game(game);
        return (StatusCode::BAD_REQUEST, Json(game.clone()));
    }

    if game.is_bust || game.can_roll {
        game.message = "Roll first, then select scoring dice from that roll.".to_string();
        save_game(game);
        return (StatusCode::BAD_REQUEST, Json(game.clone()));
    }

    if payload.dice_ids.is_empty() {
        game.message = "Select at least one scoring die.".to_string();
        save_game(game);
        return (StatusCode::BAD_REQUEST, Json(game.clone()));
    }

    let unique_ids: HashSet<usize> = payload.dice_ids.iter().copied().collect();
    if unique_ids.len() != payload.dice_ids.len() {
        game.message = "That selection repeats a die.".to_string();
        save_game(game);
        return (StatusCode::BAD_REQUEST, Json(game.clone()));
    }

    let mut values = Vec::new();
    for id in &payload.dice_ids {
        match game.dice.iter().find(|die| die.id == *id) {
            Some(die) if die.status == DieStatus::Available => {
                if let Some(value) = die.value {
                    values.push(value);
                }
            }
            _ => {
                game.message = "That selection includes dice that cannot be selected now.".to_string();
                save_game(game);
                return (StatusCode::BAD_REQUEST, Json(game.clone()));
            }
        }
    }

    let selection_score = match score_selected_dice(&values) {
        Some(score) if score > 0 => score,
        _ => {
            game.message = "Those dice are not a complete scoring selection.".to_string();
            save_game(game);
            return (StatusCode::BAD_REQUEST, Json(game.clone()));
        }
    };

    for id in &payload.dice_ids {
        if let Some(die) = game.dice.iter_mut().find(|die| die.id == *id) {
            die.status = DieStatus::Locked;
        }
    }

    game.turn_score += selection_score;
    game.selected_dice = payload.dice_ids.clone();
    game.locked_dice = game
        .dice
        .iter()
        .filter(|die| die.status == DieStatus::Locked)
        .map(|die| die.id)
        .collect();
    game.is_hot_dice = locked_count(game) == 6;
    game.can_roll = true;
    game.can_bank = game.turn_score > 0;
    game.is_bust = false;
    game.message = if game.is_hot_dice {
        format!(
            "Hot dice! Added {selection_score}. Roll all six dice again or bank {turn}.",
            turn = game.turn_score
        )
    } else {
        format!("Added {selection_score}. Roll again or bank {turn}.", turn = game.turn_score)
    };
    push_log(game, &format!("Kept {:?} for {} points.", values, selection_score));
    save_game(game);
    (StatusCode::OK, Json(game.clone()))
}

async fn api_bank(State(state): State<AppState>) -> impl IntoResponse {
    let mut guard = state.game.lock().expect("game lock poisoned");
    let game = &mut *guard;

    if !game.can_bank || game.turn_score == 0 || game.is_bust {
        game.message = "There is no valid turn score to bank.".to_string();
        save_game(game);
        return (StatusCode::BAD_REQUEST, Json(game.clone()));
    }

    game.total_score += game.turn_score;
    let banked = game.turn_score;
    clear_turn_dice(game);
    game.can_bank = false;
    game.can_roll = true;
    game.is_won = game.total_score >= game.target_score;
    game.message = if game.is_won {
        format!("You banked {banked} and won with {} points!", game.total_score)
    } else {
        format!("Banked {banked}. Roll to start the next turn.")
    };
    push_log(game, &format!("Banked {}. Total is {}.", banked, game.total_score));
    save_game(game);
    (StatusCode::OK, Json(game.clone()))
}

fn new_game_state() -> GameState {
    GameState {
        total_score: 0,
        turn_score: 0,
        target_score: TARGET_SCORE,
        dice: (0..6)
            .map(|id| Die {
                id,
                value: None,
                status: DieStatus::Empty,
            })
            .collect(),
        selected_dice: Vec::new(),
        locked_dice: Vec::new(),
        can_roll: true,
        can_bank: false,
        is_bust: false,
        is_hot_dice: false,
        is_won: false,
        message: "Ready. Roll six dice to start.".to_string(),
        log: Vec::new(),
    }
}

fn clear_turn_dice(game: &mut GameState) {
    game.turn_score = 0;
    game.selected_dice.clear();
    game.locked_dice.clear();
    game.is_bust = false;
    game.is_hot_dice = false;
    for die in &mut game.dice {
        die.value = None;
        die.status = DieStatus::Empty;
    }
}

fn locked_count(game: &GameState) -> usize {
    game.dice
        .iter()
        .filter(|die| die.status == DieStatus::Locked)
        .count()
}

fn push_log(game: &mut GameState, entry: &str) {
    game.log.insert(0, entry.to_string());
    game.log.truncate(LOG_LIMIT);
}

fn ensure_data_dir() {
    if let Err(error) = fs::create_dir_all("data") {
        eprintln!("Could not create data directory: {error}");
    }
}

fn load_game() -> Option<GameState> {
    let path = Path::new(SAVE_PATH);
    let text = fs::read_to_string(path).ok()?;
    serde_json::from_str::<GameState>(&text).ok()
}

fn save_game(game: &GameState) {
    ensure_data_dir();
    match serde_json::to_string_pretty(game) {
        Ok(text) => {
            if let Err(error) = fs::write(SAVE_PATH, text) {
                eprintln!("Could not save game: {error}");
            }
        }
        Err(error) => eprintln!("Could not serialize game: {error}"),
    }
}

fn score_selected_dice(values: &[u8]) -> Option<u32> {
    if values.is_empty() {
        return None;
    }
    let mut counts = counts_by_face(values);
    if values.len() == 6 {
        if is_straight(&counts) {
            return Some(1500);
        }
        if is_two_triplets(&counts) {
            return Some(2500);
        }
        if is_three_pairs(&counts) || is_four_kind_plus_pair(&counts) {
            return Some(1500);
        }
    }

    let mut score = 0;
    for face in 1..=6 {
        let count = counts[&face];
        if count >= 3 {
            score += kind_score(face, count);
            counts.insert(face, 0);
        }
    }

    let ones = counts[&1];
    let fives = counts[&5];
    score += ones as u32 * 100;
    score += fives as u32 * 50;
    counts.insert(1, 0);
    counts.insert(5, 0);

    if counts.values().any(|count| *count > 0) {
        None
    } else {
        Some(score)
    }
}

fn has_any_score(values: &[u8]) -> bool {
    if values.iter().any(|value| *value == 1 || *value == 5) {
        return true;
    }
    let counts = counts_by_face(values);
    if counts.values().any(|count| *count >= 3) {
        return true;
    }
    values.len() == 6
        && (is_straight(&counts)
            || is_three_pairs(&counts)
            || is_two_triplets(&counts)
            || is_four_kind_plus_pair(&counts))
}

fn counts_by_face(values: &[u8]) -> HashMap<u8, usize> {
    let mut counts = HashMap::new();
    for face in 1..=6 {
        counts.insert(face, 0);
    }
    for value in values {
        if let Some(count) = counts.get_mut(value) {
            *count += 1;
        }
    }
    counts
}

fn kind_score(face: u8, count: usize) -> u32 {
    let base = if face == 1 { 1000 } else { face as u32 * 100 };
    match count {
        3 => base,
        4 => base * 2,
        5 => base * 4,
        6 => base * 8,
        _ => 0,
    }
}

fn is_straight(counts: &HashMap<u8, usize>) -> bool {
    (1..=6).all(|face| counts[&face] == 1)
}

fn is_three_pairs(counts: &HashMap<u8, usize>) -> bool {
    counts.values().filter(|count| **count == 2).count() == 3
}

fn is_two_triplets(counts: &HashMap<u8, usize>) -> bool {
    counts.values().filter(|count| **count == 3).count() == 2
}

fn is_four_kind_plus_pair(counts: &HashMap<u8, usize>) -> bool {
    counts.values().any(|count| *count == 4) && counts.values().any(|count| *count == 2)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_one_scores() {
        assert_eq!(score_selected_dice(&[1]), Some(100));
    }

    #[test]
    fn single_five_scores() {
        assert_eq!(score_selected_dice(&[5]), Some(50));
    }

    #[test]
    fn three_ones_score() {
        assert_eq!(score_selected_dice(&[1, 1, 1]), Some(1000));
    }

    #[test]
    fn three_twos_score() {
        assert_eq!(score_selected_dice(&[2, 2, 2]), Some(200));
    }

    #[test]
    fn six_of_a_kind_scores() {
        assert_eq!(score_selected_dice(&[2, 2, 2, 2, 2, 2]), Some(1600));
        assert_eq!(score_selected_dice(&[1, 1, 1, 1, 1, 1]), Some(8000));
    }

    #[test]
    fn straight_scores() {
        assert_eq!(score_selected_dice(&[1, 2, 3, 4, 5, 6]), Some(1500));
    }

    #[test]
    fn three_pairs_score() {
        assert_eq!(score_selected_dice(&[1, 1, 3, 3, 6, 6]), Some(1500));
    }

    #[test]
    fn two_triplets_score() {
        assert_eq!(score_selected_dice(&[2, 2, 2, 4, 4, 4]), Some(2500));
    }

    #[test]
    fn four_kind_plus_pair_scores() {
        assert_eq!(score_selected_dice(&[3, 3, 3, 3, 5, 5]), Some(1500));
    }

    #[test]
    fn bust_roll_detected() {
        assert!(!has_any_score(&[2, 3, 4, 6]));
    }

    #[test]
    fn hot_dice_case_scores_all_six() {
        assert_eq!(score_selected_dice(&[1, 1, 1, 5, 5, 5]), Some(2500));
    }

    #[test]
    fn invalid_non_scoring_selection_fails() {
        assert_eq!(score_selected_dice(&[2]), None);
        assert_eq!(score_selected_dice(&[1, 2]), None);
    }
}
