async function showStatus() {
  const statusElement = document.getElementById("status");
  const state = await browser.storage.local.get(["connected", "sitesBlocked", "lastError"]);
  if (!state.connected) {
    statusElement.className = "status error";
    statusElement.textContent = state.lastError || "Desktop app is not connected.";
    return;
  }
  statusElement.className = state.sitesBlocked ? "status" : "status off";
  statusElement.textContent = state.sitesBlocked
    ? "Distracting sites are blocked."
    : "Site blocking is off.";
}

async function refreshStatus() {
  try {
    await browser.runtime.sendMessage({ type: "sync" });
  } catch (error) {
    console.error("Could not sync with Pomodoro Focus:", error);
  }
  await showStatus();
}

void refreshStatus();
