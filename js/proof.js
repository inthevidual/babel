// Spelling and grammar through LanguageTool. Only edited paragraphs are sent.
// Paragraphs are batched into one request and the free API's limits
// (20 requests and 75 kB per minute, 20 kB per request) are respected with a
// paced queue and exponential back-off on 429.

const MAX_BATCH = 12000;
const MIN_INTERVAL = 3200;

export class Proofer {
  constructor({ getText, onResult, onStatus }) {
    this.getText = getText;      // pid -> string | null (null = skip)
    this.onResult = onResult;    // (pid, text, matches[])
    this.onStatus = onStatus;    // ('idle'|'busy'|'error'|'off', detail)
    this.dirty = new Set();
    this.timer = null;
    this.last = 0;
    this.backoff = 0;
    this.running = false;
    this.config = { enabled: true, lang: null, endpoint: 'https://api.languagetool.org', username: '', apiKey: '', picky: false };
  }

  configure(cfg) {
    Object.assign(this.config, cfg);
    if (!this.active) this.onStatus('off', this.config.lang ? 'Proofing is turned off' : 'No proofing for this language');
    else this.schedule(0);
  }

  get active() { return this.config.enabled && !!this.config.lang; }

  queue(pid, delay = 1500) {
    this.dirty.add(pid);
    this.schedule(delay);
  }

  schedule(delay) {
    if (!this.active || !this.dirty.size) return;
    clearTimeout(this.timer);
    const wait = Math.max(delay, this.last + MIN_INTERVAL + this.backoff - Date.now());
    this.timer = setTimeout(() => this.flush(), wait);
  }

  async flush() {
    if (this.running || !this.active) return;
    const batch = [];
    let size = 0;
    for (const pid of this.dirty) {
      const text = this.getText(pid);
      if (text == null || !/\p{L}/u.test(text)) { this.dirty.delete(pid); this.onResult(pid, text ?? '', []); continue; }
      if (size && size + text.length > MAX_BATCH) break;
      batch.push({ pid, text, start: size });
      size += text.length + 2;
      this.dirty.delete(pid);
      if (text.length > MAX_BATCH) break;
    }
    if (!batch.length) return;
    this.running = true;
    this.last = Date.now();
    this.onStatus('busy', `Checking ${batch.length} paragraph${batch.length > 1 ? 's' : ''}…`);
    try {
      const matches = await this.request(batch.map(b => b.text).join('\n\n'));
      this.backoff = 0;
      for (const b of batch) {
        const end = b.start + b.text.length;
        const own = matches
          .filter(m => m.offset >= b.start && m.offset + m.length <= end)
          .map(m => ({ ...m, offset: m.offset - b.start }));
        this.onResult(b.pid, b.text, own);
      }
      this.onStatus('idle', '');
    } catch (e) {
      for (const b of batch) this.dirty.add(b.pid);
      if (e.status === 429 || e.status === 503) {
        this.backoff = Math.min(60000, (this.backoff || 4000) * 2);
        this.onStatus('busy', 'LanguageTool rate limit — retrying shortly');
      } else {
        this.backoff = Math.min(120000, (this.backoff || 10000) * 2);
        this.onStatus('error', e.message || 'LanguageTool is unreachable');
      }
    } finally {
      this.running = false;
      this.schedule(0);
    }
  }

  async request(text) {
    const c = this.config;
    const body = new URLSearchParams({ text, language: c.lang, level: c.picky ? 'picky' : 'default' });
    if (c.username && c.apiKey) { body.set('username', c.username); body.set('apiKey', c.apiKey); }
    const res = await fetch(c.endpoint.replace(/\/+$/, '') + '/v2/check', {
      method: 'POST',
      headers: { 'Content-Type': 'application/x-www-form-urlencoded', Accept: 'application/json' },
      body,
    });
    if (!res.ok) {
      const err = new Error(`LanguageTool answered ${res.status}`);
      err.status = res.status;
      throw err;
    }
    const json = await res.json();
    return (json.matches ?? []).map(m => ({
      offset: m.offset,
      length: m.length,
      message: m.message,
      short: m.shortMessage,
      replacements: (m.replacements ?? []).slice(0, 6).map(r => r.value),
      rule: m.rule?.id,
      kind: kindOf(m.rule),
    }));
  }
}

const kindOf = rule => {
  const t = rule?.issueType;
  if (t === 'misspelling') return 'spelling';
  if (t === 'style' || t === 'locale-violation' || t === 'register' || rule?.category?.id === 'STYLE') return 'style';
  if (t === 'typographical' || t === 'whitespace') return 'typography';
  return 'grammar';
};
