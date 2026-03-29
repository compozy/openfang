// OpenFang Schedules Page — schedule v2 authoring on the v1 API
'use strict';

var SCHEDULE_UI_ACTION_OPTIONS = [
  { value: 'agent_message', label: 'Agent Message' },
  { value: 'workflow_start', label: 'Workflow Start' },
  { value: 'workflow_signal', label: 'Workflow Signal' }
];

var SCHEDULE_UI_CRON_PRESETS = [
  { label: 'Every minute', cron: '* * * * *' },
  { label: 'Every 5 minutes', cron: '*/5 * * * *' },
  { label: 'Every 15 minutes', cron: '*/15 * * * *' },
  { label: 'Every 30 minutes', cron: '*/30 * * * *' },
  { label: 'Every hour', cron: '0 * * * *' },
  { label: 'Every 6 hours', cron: '0 */6 * * *' },
  { label: 'Daily at midnight', cron: '0 0 * * *' },
  { label: 'Daily at 9am', cron: '0 9 * * *' },
  { label: 'Weekdays at 9am', cron: '0 9 * * 1-5' },
  { label: 'Every Monday 9am', cron: '0 9 * * 1' },
  { label: 'First of month', cron: '0 0 1 * *' }
];

var SCHEDULE_UI_TIMEZONES = [
  'UTC',
  'America/New_York',
  'America/Chicago',
  'America/Denver',
  'America/Los_Angeles',
  'America/Sao_Paulo',
  'Europe/London',
  'Europe/Berlin',
  'Europe/Paris',
  'Asia/Tokyo',
  'Asia/Shanghai',
  'Asia/Kolkata',
  'Australia/Sydney',
  'Pacific/Auckland'
];

function scheduleUiNormalizeText(value) {
  return value === undefined || value === null ? '' : String(value);
}

function scheduleUiNormalizeList(payload) {
  if (Array.isArray(payload)) return payload;
  if (payload && Array.isArray(payload.items)) return payload.items;
  return [];
}

function scheduleUiFormatJson(value) {
  return JSON.stringify(value, null, 2);
}

function scheduleUiParseJson(text, fallbackValue) {
  if (!scheduleUiNormalizeText(text).trim()) {
    return fallbackValue;
  }
  return JSON.parse(text);
}

function scheduleUiDefaultForm() {
  return {
    id: '',
    name: '',
    description: '',
    enabled: true,
    cron_expr: '0 * * * *',
    timezone: 'UTC',
    one_shot: false,
    action_kind: 'workflow_start',
    action_agent: '',
    action_message_text: '',
    action_metadata_text: '{\n}',
    action_workflow: '',
    action_input_text: '{\n}',
    action_signal: '',
    action_selector_workflow_id: '',
    action_payload_text: '{\n}'
  };
}

function scheduleUiBuildAction(form) {
  var kind = scheduleUiNormalizeText(form.action_kind) || 'workflow_start';
  var message;
  var metadata;
  var inputPayload;
  var signalPayload;

  if (kind === 'agent_message') {
    message = scheduleUiNormalizeText(form.action_message_text).trim();
    var action = {
      kind: 'agent_message',
      agent: scheduleUiNormalizeText(form.action_agent).trim(),
      input: {
        items: message ? [{ type: 'text', text: message }] : []
      }
    };

    if (scheduleUiNormalizeText(form.action_metadata_text).trim()) {
      metadata = scheduleUiParseJson(form.action_metadata_text, {});
      action.metadata = metadata;
    }

    return action;
  }

  if (kind === 'workflow_signal') {
    signalPayload = scheduleUiParseJson(form.action_payload_text, {});
    return {
      kind: 'workflow_signal',
      signal: scheduleUiNormalizeText(form.action_signal).trim(),
      selector: {
        workflow_id: scheduleUiNormalizeText(form.action_selector_workflow_id).trim()
      },
      payload: signalPayload
    };
  }

  inputPayload = scheduleUiParseJson(form.action_input_text, {});
  return {
    kind: 'workflow_start',
    workflow: scheduleUiNormalizeText(form.action_workflow).trim(),
    input: inputPayload
  };
}

function scheduleUiBuildDefinition(form) {
  return {
    id: scheduleUiNormalizeText(form.id).trim(),
    name: scheduleUiNormalizeText(form.name).trim(),
    description: scheduleUiNormalizeText(form.description),
    enabled: form.enabled !== false,
    schedule: {
      kind: 'cron',
      expr: scheduleUiNormalizeText(form.cron_expr).trim(),
      timezone: scheduleUiNormalizeText(form.timezone).trim() || 'UTC'
    },
    one_shot: !!form.one_shot,
    action: scheduleUiBuildAction(form)
  };
}

function scheduleUiMessageTextFromItems(input) {
  var items = input && Array.isArray(input.items) ? input.items : [];

  return items
    .map(function(item) {
      if (!item || item.item_type !== 'text') return '';
      return scheduleUiNormalizeText(item.text);
    })
    .filter(function(value) {
      return value.length > 0;
    })
    .join('\n\n');
}

function scheduleUiHydrateForm(resource) {
  var definition = resource && resource.definition ? resource.definition : resource;
  var form = scheduleUiDefaultForm();
  var schedule = definition && definition.schedule ? definition.schedule : {};
  var action = definition && definition.action ? definition.action : { kind: 'workflow_start' };
  var selector = action && action.selector ? action.selector : {};

  form.id = scheduleUiNormalizeText(definition && definition.id);
  form.name = scheduleUiNormalizeText(definition && definition.name);
  form.description = scheduleUiNormalizeText(definition && definition.description);
  form.enabled = definition && definition.enabled !== false;
  form.one_shot = !!(definition && definition.one_shot);

  form.cron_expr = scheduleUiNormalizeText(schedule.expr) || '0 * * * *';
  form.timezone = scheduleUiNormalizeText(schedule.timezone) || 'UTC';

  form.action_kind = scheduleUiNormalizeText(action.kind) || 'workflow_start';

  if (form.action_kind === 'agent_message') {
    form.action_agent = scheduleUiNormalizeText(action.agent);
    form.action_message_text = scheduleUiMessageTextFromItems(action.input);
    form.action_metadata_text = action.metadata
      ? scheduleUiFormatJson(action.metadata)
      : '{\n}';
  } else if (form.action_kind === 'workflow_signal') {
    form.action_signal = scheduleUiNormalizeText(action.signal);
    form.action_selector_workflow_id = scheduleUiNormalizeText(selector.workflow_id);
    form.action_payload_text = scheduleUiFormatJson(
      action.payload ? action.payload : {}
    );
  } else {
    form.action_workflow = scheduleUiNormalizeText(action.workflow);
    form.action_input_text = scheduleUiFormatJson(
      action.input ? action.input : {}
    );
  }

  return form;
}

function scheduleUiOriginLabel(origin) {
  if (!origin || !origin.kind) return 'unknown';
  if (origin.kind === 'pack') {
    return origin.pack_id ? 'pack:' + origin.pack_id : 'pack';
  }
  return origin.kind;
}

function scheduleUiActionKindLabel(value) {
  var kind = typeof value === 'string' ? value : (value && value.kind ? value.kind : '');
  if (kind === 'agent_message') return 'Agent Message';
  if (kind === 'workflow_start') return 'Workflow Start';
  if (kind === 'workflow_signal') return 'Workflow Signal';
  return kind || 'Unknown';
}

function scheduleUiActionSummary(action) {
  if (!action || !action.kind) return 'No action';

  if (action.kind === 'agent_message') {
    return 'Agent ' + scheduleUiNormalizeText(action.agent || '(missing)');
  }

  if (action.kind === 'workflow_signal') {
    return 'Signal ' + scheduleUiNormalizeText(action.signal || '(missing)') +
      ' -> ' + scheduleUiNormalizeText(
        action.selector && action.selector.workflow_id
          ? action.selector.workflow_id
          : '(missing workflow)'
      );
  }

  return 'Workflow ' + scheduleUiNormalizeText(action.workflow || '(missing)');
}

function scheduleUiDescribeCron(expr) {
  if (!expr) return '';
  if (expr.indexOf('every ') === 0) return expr;
  if (expr.indexOf('at ') === 0) return 'One-time: ' + expr.substring(3);

  var map = {
    '* * * * *': 'Every minute',
    '*/5 * * * *': 'Every 5 minutes',
    '*/15 * * * *': 'Every 15 minutes',
    '*/30 * * * *': 'Every 30 minutes',
    '0 * * * *': 'Every hour',
    '0 */6 * * *': 'Every 6 hours',
    '0 0 * * *': 'Daily at midnight',
    '0 9 * * *': 'Daily at 9:00 AM',
    '0 9 * * 1-5': 'Weekdays at 9:00 AM',
    '0 9 * * 1': 'Mondays at 9:00 AM',
    '0 0 1 * *': '1st of every month'
  };
  if (map[expr]) return map[expr];

  var parts = expr.split(' ');
  if (parts.length !== 5) return expr;

  var min = parts[0];
  var hour = parts[1];
  var dom = parts[2];
  var mon = parts[3];
  var dow = parts[4];

  if (min.indexOf('*/') === 0 && hour === '*' && dom === '*' && mon === '*' && dow === '*') {
    return 'Every ' + min.substring(2) + ' minutes';
  }
  if (min === '0' && hour.indexOf('*/') === 0 && dom === '*' && mon === '*' && dow === '*') {
    return 'Every ' + hour.substring(2) + ' hours';
  }

  var dowNames = {
    '0': 'Sun', '1': 'Mon', '2': 'Tue', '3': 'Wed',
    '4': 'Thu', '5': 'Fri', '6': 'Sat', '7': 'Sun',
    '1-5': 'Weekdays', '0,6': 'Weekends', '6,0': 'Weekends'
  };

  if (dom === '*' && mon === '*' && min.match(/^\d+$/) && hour.match(/^\d+$/)) {
    var h = parseInt(hour, 10);
    var m = parseInt(min, 10);
    var ampm = h >= 12 ? 'PM' : 'AM';
    var h12 = h === 0 ? 12 : (h > 12 ? h - 12 : h);
    var mStr = m < 10 ? '0' + m : '' + m;
    var timeStr = h12 + ':' + mStr + ' ' + ampm;
    if (dow === '*') return 'Daily at ' + timeStr;
    var dowLabel = dowNames[dow] || ('DoW ' + dow);
    return dowLabel + ' at ' + timeStr;
  }

  return expr;
}

function scheduleUiIssueSeverityClass(issue) {
  return issue && issue.severity === 'error' ? 'badge badge-error' : 'badge badge-warn';
}

function schedulesPage() {
  return {
    schedules: [],
    selectedScheduleId: '',
    selectedSchedule: null,
    selectedRuntime: null,
    loading: true,
    loadError: '',
    detailLoading: false,
    detailError: '',
    searchQuery: '',
    filterEnabled: '',
    filterActionKind: '',
    saving: false,
    validating: false,
    togglingId: '',
    deletingId: '',
    forkingId: '',
    runningNowId: '',
    dryRunning: false,
    form: scheduleUiDefaultForm(),
    validationResult: null,
    dryRunResult: null,
    scheduleChangeHandler: null,

    init() {
      var self = this;
      this.scheduleChangeHandler = function() {
        self.loadSchedules({ preserveSelection: true, refreshDetail: true });
      };
      window.addEventListener('openfang:schedules-changed', this.scheduleChangeHandler);
      this.loadData();
    },

    destroy() {
      if (this.scheduleChangeHandler) {
        window.removeEventListener('openfang:schedules-changed', this.scheduleChangeHandler);
        this.scheduleChangeHandler = null;
      }
    },

    get actionKindOptions() {
      return SCHEDULE_UI_ACTION_OPTIONS;
    },

    get cronPresets() {
      return SCHEDULE_UI_CRON_PRESETS;
    },

    get timezoneOptions() {
      return SCHEDULE_UI_TIMEZONES;
    },

    get filteredSchedules() {
      var self = this;
      var needle = scheduleUiNormalizeText(this.searchQuery).trim().toLowerCase();

      return this.schedules.filter(function(item) {
        var runtime = item && item.runtime_status ? item.runtime_status : {};
        var enabledState = runtime.enabled !== undefined ? runtime.enabled : !!item.enabled;
        var actionKind = item && item.action ? item.action.kind : '';
        var haystack;

        if (self.filterEnabled === 'enabled' && !enabledState) return false;
        if (self.filterEnabled === 'disabled' && enabledState) return false;
        if (self.filterActionKind && self.filterActionKind !== actionKind) return false;

        if (!needle) return true;

        haystack = [
          item.id,
          item.name,
          item.updated_at,
          scheduleUiDescribeCron(item.schedule && item.schedule.expr),
          scheduleUiActionSummary(item.action),
          scheduleUiOriginLabel(item.origin)
        ]
          .join(' ')
          .toLowerCase();

        return haystack.indexOf(needle) >= 0;
      });
    },

    get validationIssues() {
      if (!this.validationResult || !Array.isArray(this.validationResult.issues)) {
        return [];
      }
      return this.validationResult.issues;
    },

    get canDeleteSelected() {
      return !!(
        this.selectedSchedule &&
        this.selectedScheduleId &&
        this.selectedSchedule.origin &&
        this.selectedSchedule.origin.kind !== 'pack'
      );
    },

    get canForkSelected() {
      return !!(
        this.selectedSchedule &&
        this.selectedScheduleId &&
        this.selectedSchedule.origin &&
        this.selectedSchedule.origin.kind === 'pack'
      );
    },

    get canSaveSelected() {
      if (!this.selectedScheduleId) return true;
      return !!(
        this.selectedSchedule &&
        this.selectedSchedule.origin &&
        this.selectedSchedule.origin.kind !== 'pack'
      );
    },

    get selectedRuntimeSnapshot() {
      if (this.selectedRuntime) return this.selectedRuntime;
      if (!this.selectedSchedule) return null;
      return {
        schedule_id: this.selectedSchedule.id,
        enabled: this.selectedSchedule.enabled,
        consecutive_errors: 0,
        last_status: null,
        last_run_at: null,
        next_run_at: null,
        one_shot: this.selectedSchedule.one_shot || false
      };
    },

    async loadData() {
      this.loading = true;
      this.loadError = '';
      await this.loadSchedules({ preserveSelection: true });
      this.loading = false;
    },

    sortSchedules(items) {
      return (items || []).slice().sort(function(left, right) {
        var leftUpdated = Date.parse(left && left.updated_at ? left.updated_at : '');
        var rightUpdated = Date.parse(right && right.updated_at ? right.updated_at : '');

        if (!isNaN(leftUpdated) && !isNaN(rightUpdated) && leftUpdated !== rightUpdated) {
          return rightUpdated - leftUpdated;
        }

        return scheduleUiNormalizeText(left && left.name).localeCompare(
          scheduleUiNormalizeText(right && right.name)
        );
      });
    },

    async loadSchedules(options) {
      var items;
      var response;
      var nextSelection = '';
      var i;

      options = options || {};
      this.loadError = '';

      try {
        response = await OpenFangAPI.v1.schedules.list();
        items = this.sortSchedules(scheduleUiNormalizeList(response));
        this.schedules = items;

        if (options.preserveSelection && this.selectedScheduleId) {
          for (i = 0; i < items.length; i++) {
            if (items[i].id === this.selectedScheduleId) {
              nextSelection = this.selectedScheduleId;
              break;
            }
          }
        }

        if (!nextSelection && items.length) {
          nextSelection = items[0].id;
        }

        if (nextSelection) {
          await this.selectSchedule(nextSelection, !!options.refreshDetail);
        } else {
          this.createScheduleDraft();
        }
      } catch (e) {
        this.schedules = [];
        this.loadError = e.message || 'Could not load schedules.';
      }
    },

    createScheduleDraft() {
      this.selectedScheduleId = '';
      this.selectedSchedule = null;
      this.selectedRuntime = null;
      this.detailLoading = false;
      this.detailError = '';
      this.validationResult = null;
      this.dryRunResult = null;
      this.form = scheduleUiDefaultForm();
    },

    async selectSchedule(id, forceReload) {
      var responses;

      if (!id) {
        this.createScheduleDraft();
        return;
      }
      if (!forceReload && this.selectedScheduleId === id && this.selectedSchedule) {
        return;
      }

      this.selectedScheduleId = id;
      this.detailLoading = true;
      this.detailError = '';
      this.validationResult = null;
      this.dryRunResult = null;

      try {
        responses = await Promise.all([
          OpenFangAPI.v1.schedules.get(id),
          OpenFangAPI.v1.schedules.runtime(id)
        ]);
        this.selectedSchedule = responses[0];
        this.selectedRuntime = responses[1];
        this.form = scheduleUiHydrateForm(this.selectedSchedule);
      } catch (e) {
        this.detailError = e.message || 'Could not load schedule detail.';
      }

      this.detailLoading = false;
    },

    buildDefinitionOrToast() {
      try {
        return scheduleUiBuildDefinition(this.form);
      } catch (e) {
        e.scheduleBuildError = true;
        OpenFangToast.error('Schedule definition is invalid: ' + e.message);
        throw e;
      }
    },

    issuesForPath(prefix) {
      return this.validationIssues.filter(function(issue) {
        return scheduleUiNormalizeText(issue.path).indexOf(prefix) === 0;
      });
    },

    issueSeverityClass(issue) {
      return scheduleUiIssueSeverityClass(issue);
    },

    originBadgeClass(origin) {
      return origin && origin.kind === 'pack' ? 'badge badge-info' : 'badge badge-dim';
    },

    originLabel(origin) {
      return scheduleUiOriginLabel(origin);
    },

    actionKindLabel(action) {
      return scheduleUiActionKindLabel(action);
    },

    actionSummary(action) {
      return scheduleUiActionSummary(action);
    },

    describeCron(expr) {
      return scheduleUiDescribeCron(expr);
    },

    applyCronPreset(preset) {
      this.form.cron_expr = preset.cron;
    },

    runtimeForListItem(item) {
      return item && item.runtime_status ? item.runtime_status : {
        enabled: item ? item.enabled : false,
        consecutive_errors: 0,
        last_status: null,
        last_run_at: null,
        next_run_at: null
      };
    },

    formatDateTime(value) {
      return value ? OpenFangUtils.formatDateTime(value) : '';
    },

    relativeTime(value) {
      return value ? OpenFangUtils.timeAgo(value) : '';
    },

    lastRunLabel(item) {
      var runtime = this.runtimeForListItem(item);
      if (!runtime.last_run_at) return 'Never';
      return OpenFangUtils.formatDateTime(runtime.last_run_at);
    },

    nextRunLabel(item) {
      var runtime = this.runtimeForListItem(item);
      if (!runtime.next_run_at) return '-';
      return OpenFangUtils.timeAgo(runtime.next_run_at);
    },

    lastStatusBadge(item) {
      var runtime = this.runtimeForListItem(item);
      var status = runtime.last_status;
      if (!status) return 'badge badge-dim';
      if (status === 'success' || status === 'completed') return 'badge badge-success';
      if (status === 'error' || status === 'failed') return 'badge badge-error';
      return 'badge badge-warn';
    },

    openRunHistory() {
      location.hash = 'runs';
    },

    async saveSchedule() {
      var definition;
      var saved;

      if (!this.canSaveSelected) {
        OpenFangToast.warn('Pack-managed schedules are read-only. Fork the schedule first.');
        return;
      }

      this.saving = true;

      try {
        definition = this.buildDefinitionOrToast();

        if (this.selectedScheduleId) {
          saved = await OpenFangAPI.v1.schedules.update(this.selectedScheduleId, definition);
          OpenFangToast.success('Schedule updated: ' + saved.name);
        } else {
          saved = await OpenFangAPI.v1.schedules.create(definition);
          OpenFangToast.success('Schedule created: ' + saved.name);
        }

        window.dispatchEvent(new CustomEvent('openfang:schedules-changed'));
        await this.selectSchedule(saved.id, true);
      } catch (e) {
        if (!e || !e.scheduleBuildError) {
          OpenFangToast.error('Failed to save schedule: ' + e.message);
        }
      }

      this.saving = false;
    },

    confirmDeleteSelected() {
      var self = this;

      if (!this.selectedSchedule) return;

      OpenFangUtils.confirmAction(
        'Delete Schedule',
        'Delete schedule "' + this.selectedSchedule.name + '"? This cannot be undone.',
        function() {
          self.deleteSelectedSchedule();
        }
      );
    },

    async deleteSelectedSchedule() {
      if (!this.selectedScheduleId) return;
      this.deletingId = this.selectedScheduleId;

      try {
        await OpenFangAPI.v1.schedules.delete(this.selectedScheduleId);
        OpenFangToast.success('Schedule deleted');
        this.createScheduleDraft();
        window.dispatchEvent(new CustomEvent('openfang:schedules-changed'));
      } catch (e) {
        OpenFangToast.error('Failed to delete schedule: ' + e.message);
      }

      this.deletingId = '';
    },

    async toggleScheduleEnabled(item) {
      var enabled;

      if (!item || !item.id) return;
      if (this.togglingId) return;
      if (item.origin && item.origin.kind === 'pack') {
        OpenFangToast.warn(
          'Pack-managed schedules cannot be toggled directly. Fork the schedule first.'
        );
        return;
      }

      enabled = this.runtimeForListItem(item).enabled;
      this.togglingId = item.id;

      try {
        if (enabled) {
          await OpenFangAPI.v1.schedules.disable(item.id);
        } else {
          await OpenFangAPI.v1.schedules.enable(item.id);
        }
        OpenFangToast.success(
          'Schedule ' + (enabled ? 'disabled' : 'enabled') + ': ' + item.name
        );
        window.dispatchEvent(new CustomEvent('openfang:schedules-changed'));
      } catch (e) {
        OpenFangToast.error('Failed to update schedule: ' + e.message);
      }

      this.togglingId = '';
    },

    async validateSchedule() {
      var definition;

      this.validating = true;
      this.validationResult = null;

      try {
        definition = this.buildDefinitionOrToast();
        this.validationResult = await OpenFangAPI.v1.schedules.validate({
          definition: definition,
          strict: false
        });
        if (this.validationResult.valid) {
          OpenFangToast.success('Schedule definition is valid');
        } else {
          OpenFangToast.warn('Schedule definition has validation issues');
        }
      } catch (e) {
        if (!e || !e.scheduleBuildError) {
          OpenFangToast.error('Failed to validate schedule: ' + e.message);
        }
      }

      this.validating = false;
    },

    async forkSelectedSchedule() {
      var forked;

      if (!this.selectedScheduleId) return;
      this.forkingId = this.selectedScheduleId;

      try {
        forked = await OpenFangAPI.v1.schedules.fork(this.selectedScheduleId);
        OpenFangToast.success('Schedule forked: ' + forked.name);
        window.dispatchEvent(new CustomEvent('openfang:schedules-changed'));
        await this.selectSchedule(forked.id, true);
      } catch (e) {
        OpenFangToast.error('Failed to fork schedule: ' + e.message);
      }

      this.forkingId = '';
    },

    async runNow(item) {
      var targetId = item && item.id ? item.id : this.selectedScheduleId;
      if (!targetId) {
        OpenFangToast.warn('Save the schedule before running it.');
        return;
      }

      this.runningNowId = targetId;

      try {
        await OpenFangAPI.v1.schedules.runNow(targetId);
        OpenFangToast.success('Schedule triggered: ' + (item && item.name ? item.name : 'schedule'));
        window.dispatchEvent(new CustomEvent('openfang:schedules-changed'));
      } catch (e) {
        OpenFangToast.error('Failed to run schedule: ' + e.message);
      }

      this.runningNowId = '';
    },

    async dryRun() {
      if (!this.selectedScheduleId) {
        OpenFangToast.warn('Save the schedule before running a dry-run.');
        return;
      }

      this.dryRunning = true;
      this.dryRunResult = null;

      try {
        this.dryRunResult = await OpenFangAPI.v1.schedules.dryRun(this.selectedScheduleId);
        OpenFangToast.success('Dry-run completed');
      } catch (e) {
        OpenFangToast.error('Dry-run failed: ' + e.message);
      }

      this.dryRunning = false;
    }
  };
}
