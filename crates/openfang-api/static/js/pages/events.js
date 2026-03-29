// OpenFang Events Page — event ingress tester on the v1 API
'use strict';

function eventsUiDefaultEventPayload() {
  return JSON.stringify({
    event: 'issue.created',
    source: 'api',
    payload: {},
    idempotency_key: ''
  }, null, 2);
}

function eventsPage() {
  return {
    eventJson: eventsUiDefaultEventPayload(),
    sending: false,
    dryRunning: false,
    sendResult: null,
    dryRunResult: null,
    jsonError: '',
    recentEvents: [],

    init() {
      this.eventJson = eventsUiDefaultEventPayload();
    },

    destroy() {},

    parseEventOrToast() {
      var parsed;
      this.jsonError = '';

      try {
        parsed = JSON.parse(this.eventJson);
      } catch (e) {
        this.jsonError = 'Invalid JSON: ' + e.message;
        OpenFangToast.error('Event JSON is invalid: ' + e.message);
        return null;
      }

      if (!parsed || typeof parsed !== 'object' || Array.isArray(parsed)) {
        this.jsonError = 'Event must be a JSON object.';
        OpenFangToast.error('Event must be a JSON object.');
        return null;
      }

      return parsed;
    },

    async sendEvent() {
      var body = this.parseEventOrToast();
      if (!body) return;

      this.sending = true;
      this.sendResult = null;

      try {
        this.sendResult = await OpenFangAPI.v1.events.send(body);
        OpenFangToast.success('Event sent successfully');
        this.recentEvents.unshift({
          timestamp: new Date().toISOString(),
          event: body.event || '(unknown)',
          source: body.source || '',
          mode: 'live',
          matched: this.sendResult.matched || 0,
          result: this.sendResult
        });
        if (this.recentEvents.length > 20) {
          this.recentEvents = this.recentEvents.slice(0, 20);
        }
        window.dispatchEvent(new CustomEvent('openfang:events-sent'));
      } catch (e) {
        OpenFangToast.error('Failed to send event: ' + e.message);
      }

      this.sending = false;
    },

    async dryRun() {
      var body = this.parseEventOrToast();
      if (!body) return;

      this.dryRunning = true;
      this.dryRunResult = null;

      try {
        this.dryRunResult = await OpenFangAPI.v1.events.dryRun(body);
        OpenFangToast.success('Dry-run completed');
        this.recentEvents.unshift({
          timestamp: new Date().toISOString(),
          event: body.event || '(unknown)',
          source: body.source || '',
          mode: 'dry-run',
          matched: this.dryRunResult.matched || 0,
          result: this.dryRunResult
        });
        if (this.recentEvents.length > 20) {
          this.recentEvents = this.recentEvents.slice(0, 20);
        }
      } catch (e) {
        OpenFangToast.error('Dry-run failed: ' + e.message);
      }

      this.dryRunning = false;
    },

    resetForm() {
      this.eventJson = eventsUiDefaultEventPayload();
      this.jsonError = '';
      this.sendResult = null;
      this.dryRunResult = null;
    },

    clearHistory() {
      this.recentEvents = [];
    },

    formatJson(value) {
      if (!value) return '';
      try {
        return JSON.stringify(value, null, 2);
      } catch (e) {
        return String(value);
      }
    },

    formatDateTime(value) {
      return value ? OpenFangUtils.formatDateTime(value) : '';
    },

    relativeTime(value) {
      return value ? OpenFangUtils.timeAgo(value) : '';
    },

    get matchedTriggersDryRun() {
      if (!this.dryRunResult) return [];
      return this.dryRunResult.triggers || this.dryRunResult.matched_triggers || [];
    },

    get matchedTriggersSend() {
      if (!this.sendResult) return [];
      return this.sendResult.triggers || this.sendResult.matched_triggers || [];
    },

    get dispatchedRunsSend() {
      if (!this.sendResult) return [];
      return this.sendResult.runs || this.sendResult.dispatched_runs || [];
    }
  };
}
