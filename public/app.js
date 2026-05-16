const diceEl = document.querySelector("#dice");
const messageEl = document.querySelector("#message");
const logEl = document.querySelector("#log");
const totalScoreEl = document.querySelector("#totalScore");
const turnScoreEl = document.querySelector("#turnScore");
const targetScoreEl = document.querySelector("#targetScore");
const remainingDiceEl = document.querySelector("#remainingDice");
const winBannerEl = document.querySelector("#winBanner");
const rollButton = document.querySelector("#rollButton");
const keepButton = document.querySelector("#keepButton");
const bankButton = document.querySelector("#bankButton");
const newGameButton = document.querySelector("#newGame");
const resetButton = document.querySelector("#resetButton");

let state = null;
let pendingSelection = new Set();

async function request(path, options = {}) {
  const response = await fetch(path, {
    headers: { "Content-Type": "application/json" },
    ...options,
  });
  const data = await response.json();
  state = data;
  pendingSelection.clear();
  render();
  return response.ok;
}

function dieFace(value) {
  return value ? String(value) : ".";
}

function render() {
  if (!state) return;

  totalScoreEl.textContent = state.total_score;
  turnScoreEl.textContent = state.turn_score;
  targetScoreEl.textContent = state.target_score;
  const remaining = state.dice.filter((die) => die.status !== "locked").length;
  remainingDiceEl.textContent = state.is_hot_dice ? 6 : remaining;
  messageEl.textContent = state.message;
  winBannerEl.classList.toggle("hidden", !state.is_won);

  rollButton.disabled = !state.can_roll || state.is_won;
  keepButton.disabled = pendingSelection.size === 0 || state.can_roll || state.is_bust || state.is_won;
  bankButton.disabled = !state.can_bank || state.is_bust || state.is_won;

  diceEl.innerHTML = "";
  state.dice.forEach((die) => {
    const button = document.createElement("button");
    button.type = "button";
    button.className = `die ${die.status}`;
    button.textContent = dieFace(die.value);
    button.setAttribute("aria-label", die.value ? `Die ${die.id + 1}: ${die.value}` : `Die ${die.id + 1}: empty`);
    if (pendingSelection.has(die.id)) {
      button.classList.add("selected");
    }
    button.disabled = die.status !== "available" || state.can_roll || state.is_bust || state.is_won;
    button.addEventListener("click", () => toggleDie(die.id));
    diceEl.appendChild(button);
  });

  logEl.innerHTML = "";
  state.log.forEach((entry) => {
    const item = document.createElement("li");
    item.textContent = entry;
    logEl.appendChild(item);
  });
}

async function toggleDie(id) {
  if (pendingSelection.has(id)) {
    pendingSelection.delete(id);
  } else {
    pendingSelection.add(id);
  }
  render();
}

rollButton.addEventListener("click", () => request("/api/roll", { method: "POST" }));
keepButton.addEventListener("click", () => request("/api/select", {
  method: "POST",
  body: JSON.stringify({ dice_ids: Array.from(pendingSelection) }),
}));
bankButton.addEventListener("click", () => request("/api/bank", { method: "POST" }));
newGameButton.addEventListener("click", () => request("/api/new", { method: "POST" }));
resetButton.addEventListener("click", () => request("/api/reset", { method: "POST" }));

request("/api/state").catch(() => {
  messageEl.textContent = "Could not reach the Rust server.";
});
