const API_URL = "http://127.0.0.1:5000/status";
const ALARM_NAME = "pomodoro-focus-sync";
const DOMAINS = [
  "facebook.com",
  "fb.com",
  "fbcdn.net",
  "instagram.com",
  "cdninstagram.com",
  "threads.net",
  "tiktok.com",
  "tiktokcdn.com",
  "tiktokv.com",
  "youtube.com",
  "youtu.be",
  "googlevideo.com",
  "ytimg.com"
];

async function syncRules() {
  try {
    const response = await fetch(API_URL, { cache: "no-store" });
    if (!response.ok) {
      throw new Error(`Pomodoro Focus returned HTTP ${response.status}`);
    }
    const status = await response.json();
    const rules = status.sites_blocked
      ? DOMAINS.map((domain, index) => ({
          id: index + 1,
          priority: 1,
          action: { type: "block" },
          condition: { urlFilter: `||${domain}^` }
        }))
      : [];
    const existing = await chrome.declarativeNetRequest.getDynamicRules();
    await chrome.declarativeNetRequest.updateDynamicRules({
      removeRuleIds: existing.map(rule => rule.id),
      addRules: rules
    });
    await chrome.storage.local.set({
      connected: true,
      sitesBlocked: status.sites_blocked,
      lastError: ""
    });
  } catch (error) {
    await chrome.storage.local.set({
      connected: false,
      lastError: error.message
    });
  }
}

chrome.runtime.onInstalled.addListener(() => {
  chrome.alarms.create(ALARM_NAME, { periodInMinutes: 0.5 });
  void syncRules();
});

chrome.runtime.onStartup.addListener(() => {
  chrome.alarms.create(ALARM_NAME, { periodInMinutes: 0.5 });
  void syncRules();
});

chrome.alarms.onAlarm.addListener(alarm => {
  if (alarm.name === ALARM_NAME) {
    void syncRules();
  }
});
