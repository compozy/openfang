// OpenFang Triggers Page — trigger v2 authoring on the v1 API
'use strict';

var TRIGGER_UI_TARGET_OPTIONS = [
  { value: 'agent_message', label: 'Agent Message' },
  { value: 'workflow_start', label: 'Workflow Start' },
  { value: 'workflow_signal', label: 'Workflow Signal' }
];

function triggerUiNormalizeText(value) {
  return value === undefined || value === null ? '' : String(value);
}

function triggerUiNormalizeList(payload) {
  if (Array.isArray(payload)) return payload;
  if (payload && Array.isArray(payload.items)) return payload.items;
  return [];
}

function triggerUiFormatJson(value) {
  return JSON.stringify(value, null, 2);
}

function triggerUiDefinitionMatch(definition) {
  if (!definition) return {};
  return definition.trigger_match || definition.match || {};
}

function triggerUiParseJson(text, fallbackValue) {
  if (!triggerUiNormalizeText(text).trim()) {
    return fallbackValue;
  }
  return JSON.parse(text);
}

function triggerUiSetNestedValue(target, path, value) {
  var cursor = target;
  var i;

  if (!path.length) return value;

  for (i = 0; i < path.length - 1; i++) {
    if (!cursor[path[i]] || typeof cursor[path[i]] !== 'object' || Array.isArray(cursor[path[i]])) {
      cursor[path[i]] = {};
    }
    cursor = cursor[path[i]];
  }

  cursor[path[path.length - 1]] = value;
  return target;
}

function triggerUiPayloadFromFilters(filters) {
  var payload = {};
  var filterMap = filters || {};
  var keys = Object.keys(filterMap);
  var i;

  for (i = 0; i < keys.length; i++) {
    if (keys[i].indexOf('.') >= 0) {
      triggerUiSetNestedValue(payload, keys[i].split('.'), filterMap[keys[i]]);
    } else {
      payload[keys[i]] = filterMap[keys[i]];
    }
  }

  return payload;
}

function triggerUiDefaultTriggerForm() {
  return {
    id: '',
    name: '',
    description: '',
    enabled: true,
    max_fires: 0,
    cooldown_secs: 0,
    match_event: '',
    match_source: '',
    match_contains: '',
    match_filters_text: '{\n}',
    target_kind: 'workflow_start',
    target_agent: '',
    target_message_text: '',
    target_metadata_text: '{\n}',
    target_workflow: '',
    target_input_text: '{\n}',
    target_signal: '',
    target_selector_workflow_id: '',
    target_payload_text: '{\n}'
  };
}

function triggerUiDefaultTriggerEvent(definition) {
  var triggerMatch = triggerUiDefinitionMatch(definition);
  return triggerUiFormatJson({
    event: triggerUiNormalizeText(triggerMatch.event) || 'issue.created',
    source: triggerUiNormalizeText(triggerMatch.source) || 'api',
    payload: triggerUiPayloadFromFilters(triggerMatch.filters || {})
  });
}

function triggerUiMessageTextFromItems(input) {
  var items = input && Array.isArray(input.items) ? input.items : [];

  return items
    .map(function(item) {
      if (!item || item.item_type !== 'text') return '';
      return triggerUiNormalizeText(item.text);
    })
    .filter(function(value) {
      return value.length > 0;
    })
    .join('\n\n');
}

function triggerUiBuildFilters(text) {
  var parsed = triggerUiParseJson(text, {});
  if (!parsed || Array.isArray(parsed) || typeof parsed !== 'object') {
    throw new Error('Match filters must be a JSON object');
  }
  return parsed;
}

function triggerUiBuildTarget(form) {
  var kind = triggerUiNormalizeText(form.target_kind) || 'workflow_start';
  var target;
  var message;
  var metadata;
  var inputPayload;
  var signalPayload;

  if (kind === 'agent_message') {
    message = triggerUiNormalizeText(form.target_message_text).trim();
    target = {
      kind: 'agent_message',
      agent: triggerUiNormalizeText(form.target_agent).trim(),
      input: {
        items: message ? [{ type: 'text', text: message }] : []
      }
    };

    if (triggerUiNormalizeText(form.target_metadata_text).trim()) {
      metadata = triggerUiParseJson(form.target_metadata_text, {});
      target.metadata = metadata;
    }

    return target;
  }

  if (kind === 'workflow_signal') {
    signalPayload = triggerUiParseJson(form.target_payload_text, {});
    return {
      kind: 'workflow_signal',
      signal: triggerUiNormalizeText(form.target_signal).trim(),
      selector: {
        workflow_id: triggerUiNormalizeText(form.target_selector_workflow_id).trim()
      },
      payload: signalPayload
    };
  }

  inputPayload = triggerUiParseJson(form.target_input_text, {});
  return {
    kind: 'workflow_start',
    workflow: triggerUiNormalizeText(form.target_workflow).trim(),
    input: inputPayload
  };
}

function triggerUiBuildDefinition(form) {
  return {
    id: triggerUiNormalizeText(form.id).trim(),
    name: triggerUiNormalizeText(form.name).trim(),
    description: triggerUiNormalizeText(form.description),
    enabled: form.enabled !== false,
    max_fires: Number(form.max_fires || 0),
    cooldown_secs: Number(form.cooldown_secs || 0),
    match: {
      event: triggerUiNormalizeText(form.match_event).trim() || null,
      source: triggerUiNormalizeText(form.match_source).trim() || null,
      contains: triggerUiNormalizeText(form.match_contains).trim() || null,
      filters: triggerUiBuildFilters(form.match_filters_text)
    },
    target: triggerUiBuildTarget(form)
  };
}

function triggerUiHydrateForm(resource) {
  var definition = resource && resource.definition ? resource.definition : resource;
  var form = triggerUiDefaultTriggerForm();
  var target = definition && definition.target ? definition.target : { kind: 'workflow_start' };
  var triggerMatch = triggerUiDefinitionMatch(definition);
  var selector = target && target.selector ? target.selector : {};

  form.id = triggerUiNormalizeText(definition && definition.id);
  form.name = triggerUiNormalizeText(definition && definition.name);
  form.description = triggerUiNormalizeText(definition && definition.description);
  form.enabled = definition && definition.enabled !== false;
  form.max_fires = definition && definition.max_fires ? definition.max_fires : 0;
  form.cooldown_secs = definition && definition.cooldown_secs ? definition.cooldown_secs : 0;
  form.match_event = triggerUiNormalizeText(triggerMatch.event);
  form.match_source = triggerUiNormalizeText(triggerMatch.source);
  form.match_contains = triggerUiNormalizeText(triggerMatch.contains);
  form.match_filters_text = triggerUiFormatJson(
    triggerMatch.filters ? triggerMatch.filters : {}
  );

  form.target_kind = triggerUiNormalizeText(target && target.kind) || 'workflow_start';

  if (form.target_kind === 'agent_message') {
    form.target_agent = triggerUiNormalizeText(target && target.agent);
    form.target_message_text = triggerUiMessageTextFromItems(target && target.input);
    form.target_metadata_text = target && target.metadata
      ? triggerUiFormatJson(target.metadata)
      : '{\n}';
  } else if (form.target_kind === 'workflow_signal') {
    form.target_signal = triggerUiNormalizeText(target && target.signal);
    form.target_selector_workflow_id = triggerUiNormalizeText(selector.workflow_id);
    form.target_payload_text = triggerUiFormatJson(target && target.payload ? target.payload : {});
  } else {
    form.target_workflow = triggerUiNormalizeText(target && target.workflow);
    form.target_input_text = triggerUiFormatJson(target && target.input ? target.input : {});
  }

  return form;
}

function triggerUiIssueSeverityClass(issue) {
  return issue && issue.severity === 'error' ? 'badge badge-error' : 'badge badge-warn';
}

function triggerUiOriginLabel(origin) {
  if (!origin || !origin.kind) return 'unknown';
  if (origin.kind === 'pack') {
    return origin.pack_id ? 'pack:' + origin.pack_id : 'pack';
  }
  return origin.kind;
}

function triggerUiTargetKindLabel(value) {
  var kind = typeof value === 'string' ? value : (value && value.kind ? value.kind : '');
  if (kind === 'agent_message') return 'Agent Message';
  if (kind === 'workflow_start') return 'Workflow Start';
  if (kind === 'workflow_signal') return 'Workflow Signal';
  return kind || 'Unknown';
}

function triggerUiTargetSummary(target) {
  if (!target || !target.kind) return 'No target';

  if (target.kind === 'agent_message') {
    return 'Agent ' + triggerUiNormalizeText(target.agent || '(missing)');
  }

  if (target.kind === 'workflow_signal') {
    return 'Signal ' + triggerUiNormalizeText(target.signal || '(missing)') +
      ' -> ' + triggerUiNormalizeText(
        target.selector && target.selector.workflow_id
          ? target.selector.workflow_id
          : '(missing workflow)'
      );
  }

  return 'Workflow ' + triggerUiNormalizeText(target.workflow || '(missing)');
}

function triggerUiMatchSummary(triggerMatch) {
  var parts = [];
  var filters = triggerMatch && triggerMatch.filters ? triggerMatch.filters : {};
  var filterCount = Object.keys(filters).length;

  if (triggerMatch && triggerMatch.event) {
    parts.push('event=' + triggerMatch.event);
  }
  if (triggerMatch && triggerMatch.source) {
    parts.push('source=' + triggerMatch.source);
  }
  if (triggerMatch && triggerMatch.contains) {
    parts.push('contains=' + triggerMatch.contains);
  }
  if (filterCount) {
    parts.push(filterCount + ' filter' + (filterCount === 1 ? '' : 's'));
  }

  if (!parts.length) return 'Matches any event';
  return parts.join(' · ');
}

function triggersPage() {
  return {
    triggers: [],
    selectedTriggerId: '',
    selectedTrigger: null,
    selectedRuntime: null,
    loading: true,
    loadError: '',
    detailLoading: false,
    detailError: '',
    searchQuery: '',
    filterEnabled: '',
    filterTargetKind: '',
    saving: false,
    validating: false,
    compiling: false,
    testing: false,
    togglingId: '',
    deletingId: '',
    forkingId: '',
    form: triggerUiDefaultTriggerForm(),
    validationResult: null,
    compileResult: null,
    testForm: {
      event_json: triggerUiDefaultTriggerEvent(null)
    },
    testResult: null,
    triggerChangeHandler: null,

    init() {
      var self = this;
      this.triggerChangeHandler = function() {
        self.loadTriggers({ preserveSelection: true, refreshDetail: true });
      };
      window.addEventListener('openfang:triggers-changed', this.triggerChangeHandler);
      this.loadData();
    },

    destroy() {
      if (this.triggerChangeHandler) {
        window.removeEventListener('openfang:triggers-changed', this.triggerChangeHandler);
        this.triggerChangeHandler = null;
      }
    },

    get targetKindOptions() {
      return TRIGGER_UI_TARGET_OPTIONS;
    },

    get filteredTriggers() {
      var self = this;
      var needle = triggerUiNormalizeText(this.searchQuery).trim().toLowerCase();

      return this.triggers.filter(function(item) {
        var runtime = item && item.runtime_status ? item.runtime_status : {};
        var enabledState = runtime.enabled !== undefined ? runtime.enabled : !!item.enabled;
        var targetKind = item && item.target ? item.target.kind : '';
        var haystack;

        if (self.filterEnabled === 'enabled' && !enabledState) return false;
        if (self.filterEnabled === 'disabled' && enabledState) return false;
        if (self.filterTargetKind && self.filterTargetKind !== targetKind) return false;

        if (!needle) return true;

        haystack = [
          item.id,
          item.name,
          item.updated_at,
          triggerUiMatchSummary(triggerUiDefinitionMatch(item)),
          triggerUiTargetSummary(item.target),
          triggerUiOriginLabel(item.origin)
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

    get compileSummary() {
      var triggerIr;
      if (!this.compileResult || !this.compileResult.compiled) return null;
      triggerIr = this.compileResult.compiled.trigger_ir;
      if (!triggerIr) return null;

      return {
        trigger_id: triggerIr.trigger_id || '',
        dispatch_action: triggerUiNormalizeText(triggerIr.dispatch_action),
        target_kind: triggerIr.target ? triggerUiNormalizeText(triggerIr.target.kind) : '',
        event: triggerUiNormalizeText(triggerUiDefinitionMatch(triggerIr).event)
      };
    },

    get canDeleteSelected() {
      return !!(
        this.selectedTrigger &&
        this.selectedTriggerId &&
        this.selectedTrigger.origin &&
        this.selectedTrigger.origin.kind !== 'pack'
      );
    },

    get canForkSelected() {
      return !!(
        this.selectedTrigger &&
        this.selectedTriggerId &&
        this.selectedTrigger.origin &&
        this.selectedTrigger.origin.kind === 'pack'
      );
    },

    get canSaveSelected() {
      if (!this.selectedTriggerId) return true;
      return !!(
        this.selectedTrigger &&
        this.selectedTrigger.origin &&
        this.selectedTrigger.origin.kind !== 'pack'
      );
    },

    get selectedRuntimeSnapshot() {
      if (this.selectedRuntime) return this.selectedRuntime;
      if (!this.selectedTrigger) return null;
      return {
        trigger_id: this.selectedTrigger.id,
        enabled: this.selectedTrigger.enabled,
        fire_count: 0,
        max_fires: this.selectedTrigger.max_fires,
        cooldown_secs: this.selectedTrigger.cooldown_secs,
        last_fired_at: null
      };
    },

    async loadData() {
      this.loading = true;
      this.loadError = '';
      await this.loadTriggers({ preserveSelection: true });
      this.loading = false;
    },

    sortTriggers(items) {
      return (items || []).slice().sort(function(left, right) {
        var leftUpdated = Date.parse(left && left.updated_at ? left.updated_at : '');
        var rightUpdated = Date.parse(right && right.updated_at ? right.updated_at : '');

        if (!isNaN(leftUpdated) && !isNaN(rightUpdated) && leftUpdated !== rightUpdated) {
          return rightUpdated - leftUpdated;
        }

        return triggerUiNormalizeText(left && left.name).localeCompare(
          triggerUiNormalizeText(right && right.name)
        );
      });
    },

    async loadTriggers(options) {
      var items;
      var response;
      var nextSelection = '';
      var i;

      options = options || {};
      this.loadError = '';

      try {
        response = await OpenFangAPI.v1.triggers.list({ limit: 200 });
        items = this.sortTriggers(triggerUiNormalizeList(response));
        this.triggers = items;

        if (options.preserveSelection && this.selectedTriggerId) {
          for (i = 0; i < items.length; i++) {
            if (items[i].id === this.selectedTriggerId) {
              nextSelection = this.selectedTriggerId;
              break;
            }
          }
        }

        if (!nextSelection && items.length) {
          nextSelection = items[0].id;
        }

        if (nextSelection) {
          await this.selectTrigger(nextSelection, !!options.refreshDetail);
        } else {
          this.createTriggerDraft();
        }
      } catch (e) {
        this.triggers = [];
        this.loadError = e.message || 'Could not load triggers.';
      }
    },

    createTriggerDraft() {
      this.selectedTriggerId = '';
      this.selectedTrigger = null;
      this.selectedRuntime = null;
      this.detailLoading = false;
      this.detailError = '';
      this.validationResult = null;
      this.compileResult = null;
      this.testResult = null;
      this.form = triggerUiDefaultTriggerForm();
      this.testForm = {
        event_json: triggerUiDefaultTriggerEvent(null)
      };
    },

    async selectTrigger(id, forceReload) {
      var responses;

      if (!id) {
        this.createTriggerDraft();
        return;
      }
      if (!forceReload && this.selectedTriggerId === id && this.selectedTrigger) {
        return;
      }

      this.selectedTriggerId = id;
      this.detailLoading = true;
      this.detailError = '';
      this.validationResult = null;
      this.compileResult = null;
      this.testResult = null;

      try {
        responses = await Promise.all([
          OpenFangAPI.v1.triggers.get(id),
          OpenFangAPI.v1.triggers.runtime(id)
        ]);
        this.selectedTrigger = responses[0];
        this.selectedRuntime = responses[1];
        this.form = triggerUiHydrateForm(this.selectedTrigger);
        this.testForm = {
          event_json: triggerUiDefaultTriggerEvent(this.selectedTrigger)
        };
      } catch (e) {
        this.detailError = e.message || 'Could not load trigger detail.';
      }

      this.detailLoading = false;
    },

    buildDefinitionOrToast() {
      try {
        return triggerUiBuildDefinition(this.form);
      } catch (e) {
        e.triggerBuildError = true;
        OpenFangToast.error('Trigger definition is invalid: ' + e.message);
        throw e;
      }
    },

    resetTestEventFromForm() {
      try {
        var definition = this.buildDefinitionOrToast();
        this.testForm.event_json = triggerUiDefaultTriggerEvent({
          trigger_match: definition.match
        });
      } catch (e) {
        if (!e || !e.triggerBuildError) {
          OpenFangToast.error('Failed to build default test event: ' + e.message);
        }
      }
    },

    issuesForPath(prefix) {
      return this.validationIssues.filter(function(issue) {
        return triggerUiNormalizeText(issue.path).indexOf(prefix) === 0;
      });
    },

    issueSeverityClass(issue) {
      return triggerUiIssueSeverityClass(issue);
    },

    originBadgeClass(origin) {
      return origin && origin.kind === 'pack' ? 'badge badge-info' : 'badge badge-dim';
    },

    originLabel(origin) {
      return triggerUiOriginLabel(origin);
    },

    targetKindLabel(target) {
      return triggerUiTargetKindLabel(target);
    },

    targetSummary(target) {
      return triggerUiTargetSummary(target);
    },

    matchSummary(triggerMatch) {
      return triggerUiMatchSummary(triggerMatch);
    },

    runtimeForListItem(item) {
      return item && item.runtime_status ? item.runtime_status : {
        enabled: item ? item.enabled : false,
        fire_count: 0,
        last_fired_at: null
      };
    },

    formatDateTime(value) {
      return value ? OpenFangUtils.formatDateTime(value) : '';
    },

    relativeTime(value) {
      return value ? OpenFangUtils.timeAgo(value) : '';
    },

    lastFiredLabel(item) {
      var runtime = this.runtimeForListItem(item);
      if (!runtime.last_fired_at) return 'Never';
      return OpenFangUtils.formatDateTime(runtime.last_fired_at);
    },

    openRunHistory() {
      location.hash = 'runs';
    },

    async saveTrigger() {
      var definition;
      var saved;

      if (!this.canSaveSelected) {
        OpenFangToast.warn('Pack-managed triggers are read-only. Fork the trigger first.');
        return;
      }

      this.saving = true;

      try {
        definition = this.buildDefinitionOrToast();

        if (this.selectedTriggerId) {
          saved = await OpenFangAPI.v1.triggers.update(this.selectedTriggerId, definition);
          OpenFangToast.success('Trigger updated: ' + saved.name);
        } else {
          saved = await OpenFangAPI.v1.triggers.create(definition);
          OpenFangToast.success('Trigger created: ' + saved.name);
        }

        window.dispatchEvent(new CustomEvent('openfang:triggers-changed'));
        await this.selectTrigger(saved.id, true);
      } catch (e) {
        if (!e || !e.triggerBuildError) {
          OpenFangToast.error('Failed to save trigger: ' + e.message);
        }
      }

      this.saving = false;
    },

    confirmDeleteSelected() {
      var self = this;

      if (!this.selectedTrigger) return;

      OpenFangUtils.confirmAction(
        'Delete Trigger',
        'Delete trigger "' + this.selectedTrigger.name + '"? This cannot be undone.',
        function() {
          self.deleteSelectedTrigger();
        }
      );
    },

    async deleteSelectedTrigger() {
      if (!this.selectedTriggerId) return;
      this.deletingId = this.selectedTriggerId;

      try {
        await OpenFangAPI.v1.triggers.delete(this.selectedTriggerId);
        OpenFangToast.success('Trigger deleted');
        this.createTriggerDraft();
        window.dispatchEvent(new CustomEvent('openfang:triggers-changed'));
      } catch (e) {
        OpenFangToast.error('Failed to delete trigger: ' + e.message);
      }

      this.deletingId = '';
    },

    async toggleTriggerEnabled(item) {
      var enabled;

      if (!item || !item.id) return;
      if (this.togglingId) return;
      if (item.origin && item.origin.kind === 'pack') {
        OpenFangToast.warn('Pack-managed triggers cannot be toggled directly. Fork the trigger first.');
        return;
      }

      enabled = this.runtimeForListItem(item).enabled;
      this.togglingId = item.id;

      try {
        if (enabled) {
          await OpenFangAPI.v1.triggers.disable(item.id);
        } else {
          await OpenFangAPI.v1.triggers.enable(item.id);
        }
        OpenFangToast.success(
          'Trigger ' + (enabled ? 'disabled' : 'enabled') + ': ' + item.name
        );
        window.dispatchEvent(new CustomEvent('openfang:triggers-changed'));
      } catch (e) {
        OpenFangToast.error('Failed to update trigger: ' + e.message);
      }

      this.togglingId = '';
    },

    async validateTrigger() {
      var definition;

      this.validating = true;
      this.validationResult = null;

      try {
        definition = this.buildDefinitionOrToast();
        this.validationResult = await OpenFangAPI.v1.triggers.validate({
          definition: definition,
          strict: false
        });
        if (this.validationResult.valid) {
          OpenFangToast.success('Trigger definition is valid');
        } else {
          OpenFangToast.warn('Trigger definition has validation issues');
        }
      } catch (e) {
        if (!e || !e.triggerBuildError) {
          OpenFangToast.error('Failed to validate trigger: ' + e.message);
        }
      }

      this.validating = false;
    },

    async compileTrigger() {
      var definition;

      this.compiling = true;
      this.compileResult = null;

      try {
        definition = this.buildDefinitionOrToast();
        this.compileResult = await OpenFangAPI.v1.triggers.compile({
          definition: definition
        });
        OpenFangToast.success('Trigger compiled successfully');
      } catch (e) {
        if (!e || !e.triggerBuildError) {
          OpenFangToast.error('Failed to compile trigger: ' + e.message);
        }
      }

      this.compiling = false;
    },

    async forkSelectedTrigger() {
      var forked;

      if (!this.selectedTriggerId) return;
      this.forkingId = this.selectedTriggerId;

      try {
        forked = await OpenFangAPI.v1.triggers.fork(this.selectedTriggerId, {
          mode: 'shadow'
        });
        OpenFangToast.success('Trigger forked: ' + forked.name);
        window.dispatchEvent(new CustomEvent('openfang:triggers-changed'));
        await this.selectTrigger(forked.id, true);
      } catch (e) {
        OpenFangToast.error('Failed to fork trigger: ' + e.message);
      }

      this.forkingId = '';
    },

    buildTestEventOrToast() {
      try {
        return triggerUiParseJson(this.testForm.event_json, {});
      } catch (e) {
        e.triggerTestBuildError = true;
        OpenFangToast.error('Synthetic event JSON is invalid: ' + e.message);
        throw e;
      }
    },

    async runTriggerTest() {
      var eventPayload;

      if (!this.selectedTriggerId) {
        OpenFangToast.warn('Save the trigger before running a test event.');
        return;
      }

      this.testing = true;
      this.testResult = null;

      try {
        eventPayload = this.buildTestEventOrToast();
        this.testResult = await OpenFangAPI.v1.triggers.test(this.selectedTriggerId, {
          event: eventPayload
        });
        OpenFangToast.success('Trigger test completed');
      } catch (e) {
        if (!e || !e.triggerTestBuildError) {
          OpenFangToast.error('Failed to run trigger test: ' + e.message);
        }
      }

      this.testing = false;
    },

    testExplanationLabel() {
      if (!this.testResult || !this.testResult.explanation) return '';
      return this.testResult.explanation.match || '';
    }
  };
}
