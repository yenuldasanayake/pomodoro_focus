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
    const existing = await browser.declarativeNetRequest.getDynamicRules();
    await browser.declarativeNetRequest.updateDynamicRules({
      removeRuleIds: existing.map(rule => rule.id),
      addRules: rules
    });
    await browser.storage.local.set({
      connected: true,
      sitesBlocked: status.sites_blocked,
      lastError: ""
    });
  } catch (error) {
    await browser.storage.local.set({
      connected: false,
      lastError: error.message
    });
  }
}

browser.runtime.onInstalled.addListener(() => {
  browser.alarms.create(ALARM_NAME, { periodInMinutes: 1 });
  void syncRules();
});

browser.runtime.onStartup.addListener(() => {
  browser.alarms.create(ALARM_NAME, { periodInMinutes: 1 });
  void syncRules();
});

browser.alarms.onAlarm.addListener(alarm => {
  if (alarm.name === ALARM_NAME) {
    void syncRules();
  }
});

browser.runtime.onMessage.addListener(message => {
  if (message.type === "sync") {
    return syncRules();
  }
});
