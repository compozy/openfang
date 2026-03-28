// OpenFang Runs Page — durable workflow runs with live detail streaming
'use strict';

function runsPage() {
  return {
    allRuns: [],
    runs: [],
    workflows: [],
    workflowDetails: {},
    recoveryFlags: {},
    loading: true,
    loadError: '',
    workflowLoadError: '',
    detailLoading: false,
    detailError: '',
    filterStatus: '',
    filterWorkflowId: '',
    searchQuery: '',
    selectedRunId: '',
    selectedRun: null,
    selectedTab: 'overview',
    checkpoints: [],
    signals: [],
    dispatches: [],
    hitlRequests: [],
    signalForm: {
      name: '',
      payload: '{\n  "value": ""\n}',
      source: 'dashboard'
    },
    signalSubmitting: false,
    runAction: '',
    dispatchActionId: '',
    hitlSubmittingId: '',
    hitlDrafts: {},
    eventEntries: [],
    eventStream: null,
    eventConnectionState: 'idle',
    autoScrollEvents: true,
    clockTimer: null,
    nowTick: Date.now(),
    listRequestToken: 0,
    detailRequestToken: 0,

    init() {
      var self = this;
      this.clockTimer = setInterval(function() {
        self.nowTick = Date.now();
      }, 30000);
      this.loadData();
    },

    destroy() {
      if (this.clockTimer) {
        clearInterval(this.clockTimer);
        this.clockTimer = null;
      }
      this.closeEventStream();
    },

    get workflowOptions() {
      var seen = {};
      var options = [];
      var i;

      for (i = 0; i < this.workflows.length; i++) {
        var workflow = this.workflows[i];
        if (!workflow || !workflow.id || seen[workflow.id]) continue;
        seen[workflow.id] = true;
        options.push({
          id: workflow.id,
          name: workflow.name || workflow.id
        });
      }

      for (i = 0; i < this.runs.length; i++) {
        var run = this.runs[i];
        if (!run || !run.workflow_id || seen[run.workflow_id]) continue;
        seen[run.workflow_id] = true;
        options.push({
          id: run.workflow_id,
          name: this.workflowName(run.workflow_id)
        });
      }

      return options.sort(function(left, right) {
        return left.name.localeCompare(right.name);
      });
    },

    get statusOptions() {
      return [
        { value: '', label: 'All Statuses' },
        { value: 'pending', label: 'Pending' },
        { value: 'running', label: 'Running' },
        { value: 'waiting_signal', label: 'Waiting Signal' },
        { value: 'waiting_hitl', label: 'Waiting HITL' },
        { value: 'paused', label: 'Paused' },
        { value: 'completed', label: 'Completed' },
        { value: 'failed', label: 'Failed' },
        { value: 'cancelled', label: 'Cancelled' }
      ];
    },

    get selectedWorkflow() {
      var workflowId = this.selectedRun ? this.selectedRun.workflow_id : '';
      return this.workflowResource(workflowId);
    },

    get selectedRunHasRecoveryCheckpoint() {
      return !!(this.selectedRunId && this.recoveryFlags[this.selectedRunId]);
    },

    normalizeList(payload) {
      if (Array.isArray(payload)) return payload;
      if (payload && Array.isArray(payload.items)) return payload.items;
      return [];
    },

    runQueryParams() {
      return {
        limit: 200
      };
    },

    applyRunFilters(items) {
      var query = this.searchQuery.trim().toLowerCase();

      return (items || []).filter(function(run) {
        if (!run) return false;
        if (this.filterStatus && run.status !== this.filterStatus) return false;
        if (this.filterWorkflowId && run.workflow_id !== this.filterWorkflowId) return false;
        if (!query) return true;

        var haystack = [
          run.id,
          run.workflow_id,
          Array.isArray(run.labels) ? run.labels.join(' ') : '',
          run.status
        ]
          .join(' ')
          .toLowerCase();

        return haystack.indexOf(query) >= 0;
      }, this);
    },

    async loadData() {
      this.loading = true;
      this.loadError = '';

      await Promise.all([
        this.loadWorkflows(),
        this.loadRuns({ refreshDetail: true, silent: true })
      ]);

      this.loading = false;
    },

    async loadWorkflows() {
      this.workflowLoadError = '';

      try {
        var response = await OpenFangAPI.v1.workflows.list();
        this.workflows = this.normalizeList(response);
      } catch (e) {
        this.workflows = [];
        this.workflowLoadError = e.message || 'Could not load workflows.';
      }
    },

    async loadRuns(options) {
      options = options || {};
      var requestToken = ++this.listRequestToken;

      if (!options.silent) {
        this.loading = true;
      }

      this.loadError = '';

      try {
        var response = await OpenFangAPI.v1.runs.list(this.runQueryParams());
        if (requestToken !== this.listRequestToken) return;

        var items = this.sortRuns(this.normalizeList(response));
        this.allRuns = items;
        await this.hydrateRecoveryFlags(items);

        var filteredItems = this.applyRunFilters(items);
        this.runs = filteredItems;
        var nextSelectedRunId = this.resolveSelectedRunId(filteredItems);
        if (!nextSelectedRunId) {
          this.clearSelection();
        } else if (nextSelectedRunId !== this.selectedRunId) {
          await this.selectRun(nextSelectedRunId, false);
        } else if (options.refreshDetail !== false && this.selectedRunId) {
          await this.refreshSelectedRun();
        }
      } catch (e) {
        if (requestToken !== this.listRequestToken) return;
        this.allRuns = [];
        this.runs = [];
        this.loadError = e.message || 'Could not load workflow runs.';
        this.clearSelection();
      }

      if (!options.silent) {
        this.loading = false;
      }
    },

    resolveSelectedRunId(items) {
      if (!items.length) return '';
      for (var i = 0; i < items.length; i++) {
        if (items[i].id === this.selectedRunId) return this.selectedRunId;
      }
      return items[0].id;
    },

    async selectRun(runId, keepTab) {
      if (!runId) {
        this.clearSelection();
        return;
      }
      if (runId === this.selectedRunId && this.selectedRun) {
        if (!keepTab) {
          this.selectedTab = 'overview';
        }
        return;
      }

      this.selectedRunId = runId;
      if (!keepTab) {
        this.selectedTab = 'overview';
      }
      this.detailError = '';
      this.detailLoading = true;
      this.checkpoints = [];
      this.signals = [];
      this.dispatches = [];
      this.hitlRequests = [];
      this.connectEventStream(runId);
      await this.refreshSelectedRun();
    },

    async refreshSelectedRun() {
      var runId = this.selectedRunId;
      if (!runId) return;

      var requestToken = ++this.detailRequestToken;
      this.detailLoading = true;
      this.detailError = '';

      try {
        var responses = await Promise.all([
          OpenFangAPI.v1.runs.get(runId),
          OpenFangAPI.v1.runs.checkpoints(runId),
          OpenFangAPI.v1.runs.signals(runId),
          OpenFangAPI.v1.runs.dispatches(runId),
          OpenFangAPI.v1.runs.hitlRequests(runId)
        ]);
        if (requestToken !== this.detailRequestToken || this.selectedRunId !== runId) return;

        this.selectedRun = responses[0];
        this.checkpoints = this.sortCheckpoints(this.normalizeList(responses[1]));
        this.signals = this.sortSignals(this.normalizeList(responses[2]));
        this.dispatches = this.sortDispatches(this.normalizeList(responses[3]));
        this.hitlRequests = this.sortHitlRequests(this.normalizeList(responses[4]));
        this.recoveryFlags[runId] = this.hasRecoveryCheckpoint(this.checkpoints);
        this.recoveryFlags = Object.assign({}, this.recoveryFlags);
        this.mergeRunSummary(this.selectedRun);
        await this.ensureWorkflowDetail(this.selectedRun.workflow_id);
      } catch (e) {
        if (requestToken !== this.detailRequestToken || this.selectedRunId !== runId) return;
        this.detailError = e.message || 'Could not load run details.';
      }

      if (requestToken === this.detailRequestToken && this.selectedRunId === runId) {
        this.detailLoading = false;
      }
    },

    async ensureWorkflowDetail(workflowId) {
      if (!workflowId) return null;
      if (Object.prototype.hasOwnProperty.call(this.workflowDetails, workflowId)) {
        return this.workflowDetails[workflowId];
      }

      try {
        var detail = await OpenFangAPI.v1.workflows.get(workflowId);
        this.workflowDetails[workflowId] = detail;
      } catch (e) {
        this.workflowDetails[workflowId] = null;
      }

      this.workflowDetails = Object.assign({}, this.workflowDetails);
      return this.workflowDetails[workflowId];
    },

    async hydrateRecoveryFlags(items) {
      var self = this;
      var pausedRuns = (items || []).filter(function(run) {
        return run && run.status === 'paused'
          && !Object.prototype.hasOwnProperty.call(self.recoveryFlags, run.id);
      });

      if (!pausedRuns.length) return;

      await Promise.all(pausedRuns.map(async function(run) {
        try {
          var response = await OpenFangAPI.v1.runs.checkpoints(run.id);
          self.recoveryFlags[run.id] =
            self.hasRecoveryCheckpoint(self.normalizeList(response));
        } catch (e) {
          self.recoveryFlags[run.id] = false;
        }
      }));

      this.recoveryFlags = Object.assign({}, this.recoveryFlags);
    },

    clearSelection() {
      this.selectedRunId = '';
      this.selectedRun = null;
      this.detailError = '';
      this.detailLoading = false;
      this.selectedTab = 'overview';
      this.checkpoints = [];
      this.signals = [];
      this.dispatches = [];
      this.hitlRequests = [];
      this.eventEntries = [];
      this.eventConnectionState = 'idle';
      this.closeEventStream();
    },

    sortRuns(items) {
      return (items || []).slice().sort(function(left, right) {
        return compareTimestampsDesc(left && left.updated_at, right && right.updated_at)
          || compareStrings(left && left.id, right && right.id);
      });
    },

    sortCheckpoints(items) {
      return (items || []).slice().sort(function(left, right) {
        return compareTimestampsDesc(left && left.created_at, right && right.created_at);
      });
    },

    sortSignals(items) {
      return (items || []).slice().sort(function(left, right) {
        return compareTimestampsDesc(left && left.created_at, right && right.created_at);
      });
    },

    sortDispatches(items) {
      return (items || []).slice().sort(function(left, right) {
        return compareTimestampsDesc(left && left.updated_at, right && right.updated_at);
      });
    },

    sortHitlRequests(items) {
      return (items || []).slice().sort(function(left, right) {
        var sequenceOrder = (left.sequence_no || 0) - (right.sequence_no || 0);
        if (sequenceOrder !== 0) return sequenceOrder;
        return compareTimestampsDesc(left && left.created_at, right && right.created_at);
      });
    },

    statusBadgeClass(status) {
      return OpenFangUtils.statusBadge(status);
    },

    statusLabel(status) {
      if (!status) return 'Unknown';
      return String(status)
        .replace(/_/g, ' ')
        .replace(/\b\w/g, function(letter) { return letter.toUpperCase(); });
    },

    waitingKindLabel(kind) {
      if (!kind) return '';
      if (kind === 'signal') return 'Signal';
      if (kind === 'hitl') return 'HITL';
      return this.statusLabel(kind);
    },

    workflowResource(workflowId) {
      if (!workflowId) return null;

      if (this.workflowDetails[workflowId]) {
        return this.workflowDetails[workflowId];
      }

      for (var i = 0; i < this.workflows.length; i++) {
        if (this.workflows[i].id === workflowId) return this.workflows[i];
      }

      return null;
    },

    workflowName(workflowId) {
      var workflow = this.workflowResource(workflowId);
      return workflow && workflow.name ? workflow.name : workflowId || 'Unknown workflow';
    },

    workflowDescription(workflowId) {
      var workflow = this.workflowResource(workflowId);
      return workflow && workflow.description ? workflow.description : '';
    },

    workflowStepCount(workflowId) {
      var workflow = this.workflowResource(workflowId);
      if (!workflow) return 0;
      if (Array.isArray(workflow.steps)) return workflow.steps.length;
      if (typeof workflow.steps === 'number') return workflow.steps;
      return 0;
    },

    workflowStepDefinition(run) {
      if (!run || !run.workflow_id || !run.current_step_id) return null;

      var workflow = this.workflowDetails[run.workflow_id];
      if (!workflow || !Array.isArray(workflow.steps)) return null;

      for (var i = 0; i < workflow.steps.length; i++) {
        var step = workflow.steps[i];
        if (step.id === run.current_step_id || step.name === run.current_step_id) {
          return step;
        }
      }

      return null;
    },

    currentStepIndex(run) {
      if (!run || !run.workflow_id || !run.current_step_id) return -1;

      var workflow = this.workflowDetails[run.workflow_id];
      if (!workflow || !Array.isArray(workflow.steps)) return -1;

      for (var i = 0; i < workflow.steps.length; i++) {
        var step = workflow.steps[i];
        if (step.id === run.current_step_id || step.name === run.current_step_id) {
          return i;
        }
      }

      return -1;
    },

    currentStepLabel(run) {
      if (!run) return 'Unknown';
      if (!run.current_step_id) {
        if (run.status === 'completed') return 'Workflow complete';
        if (run.status === 'pending') return 'Queued';
        return 'Not available';
      }

      var step = this.workflowStepDefinition(run);
      return step && step.name ? step.name : run.current_step_id;
    },

    progressPercent(run) {
      if (!run) return 0;
      var totalSteps = this.workflowStepCount(run.workflow_id);
      if (!totalSteps) {
        return run.status === 'completed' ? 100 : 0;
      }
      if (run.status === 'completed') return 100;

      var index = this.currentStepIndex(run);
      if (index < 0) return 0;
      var completedSteps = index + 1;
      return Math.min(100, Math.max(5, Math.round((completedSteps / totalSteps) * 100)));
    },

    progressLabel(run) {
      if (!run) return 'No run selected';
      var totalSteps = this.workflowStepCount(run.workflow_id);
      var index = this.currentStepIndex(run);
      if (!totalSteps || index < 0) {
        return this.currentStepLabel(run);
      }
      return 'Step ' + (index + 1) + ' of ' + totalSteps;
    },

    waitingSummary(run) {
      if (!run) return '';
      if (run.status === 'waiting_signal' && run.waiting_ref) {
        return 'Waiting for signal "' + run.waiting_ref + '"';
      }
      if (run.status === 'waiting_hitl') {
        if (run.active_hitl_request_id) {
          return 'Waiting for HITL request ' + run.active_hitl_request_id;
        }
        return 'Waiting for human input';
      }
      if (run.status === 'paused' && this.showRecoveryIndicator(run)) {
        return 'Recovered after restart and waiting for manual resume.';
      }
      return '';
    },

    showRecoveryIndicator(run) {
      return !!(run && run.status === 'paused' && this.recoveryFlags[run.id]);
    },

    recoveryTooltip() {
      return 'Recovered after restart. Resume the run to continue execution.';
    },

    formatDateTime(value) {
      return OpenFangUtils.formatDateTime(value);
    },

    relativeTime(value) {
      var tick = this.nowTick;
      if (tick < 0) return '';
      return OpenFangUtils.timeAgo(value);
    },

    formatJson(value) {
      if (value === undefined || value === null) return '{}';
      try {
        return JSON.stringify(value, null, 2);
      } catch (e) {
        return String(value);
      }
    },

    checkpointKindLabel(kind) {
      if (!kind) return 'Checkpoint';
      return kind.replace(/_/g, ' ');
    },

    checkpointSummary(checkpoint) {
      if (!checkpoint || !checkpoint.kind) return 'No checkpoint data.';
      var data = checkpoint.data || {};

      if (checkpoint.kind === 'run_recovered_needs_resume') {
        return 'Recovered after restart. Manual resume is required before execution can continue.';
      }
      if (checkpoint.kind === 'waiting_signal') {
        return 'Waiting for signal "' + (data.signal_name || checkpoint.step_id || 'unknown') + '".';
      }
      if (checkpoint.kind === 'hitl_requested') {
        return 'Human input requested for this step.';
      }
      if (checkpoint.kind === 'run_paused' || checkpoint.kind === 'run_resumed'
          || checkpoint.kind === 'run_cancelled') {
        return 'Triggered by ' + (data.actor_source || 'system') + '.';
      }
      if (checkpoint.kind === 'step_completed') {
        return 'Step completed successfully.';
      }
      if (checkpoint.kind === 'step_started') {
        return 'Step execution started.';
      }

      if (data && Object.keys(data).length > 0) {
        return OpenFangUtils.truncate(this.formatJson(data), 180);
      }

      return 'Checkpoint recorded.';
    },

    signalStatusClass(signal) {
      return signal && signal.consumed
        ? 'badge badge-run-completed'
        : 'badge badge-run-waiting-signal';
    },

    signalStatusLabel(signal) {
      return signal && signal.consumed ? 'Consumed' : 'Pending';
    },

    async submitSignal() {
      if (!this.selectedRunId) return;

      var name = this.signalForm.name.trim();
      var source = this.signalForm.source.trim();
      if (!name) {
        OpenFangToast.error('Signal name is required.');
        return;
      }
      if (!source) {
        OpenFangToast.error('Signal source is required.');
        return;
      }

      var payload;
      try {
        payload = JSON.parse(this.signalForm.payload || '{}');
      } catch (e) {
        OpenFangToast.error('Signal payload must be valid JSON.');
        return;
      }

      this.signalSubmitting = true;

      try {
        var response = await OpenFangAPI.v1.runs.sendSignal(this.selectedRunId, {
          name: name,
          payload: payload,
          source: source,
          idempotency_key: 'dashboard-' + Date.now()
        });
        this.upsertSignal(response);
        OpenFangToast.success('Signal sent to the run.');
        this.signalForm.payload = '{\n  "value": ""\n}';
        await this.loadRuns({ silent: true, refreshDetail: true });
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to send signal.');
      }

      this.signalSubmitting = false;
    },

    canPause(run) {
      return !!(run && (run.status === 'running' || run.status === 'waiting_signal'));
    },

    canResume(run) {
      return !!(run && run.status === 'paused');
    },

    canCancel(run) {
      return !!(run && (
        run.status === 'pending'
        || run.status === 'running'
        || run.status === 'waiting_signal'
        || run.status === 'waiting_hitl'
        || run.status === 'paused'
      ));
    },

    async executeRunAction(action) {
      if (!this.selectedRunId || !this.selectedRun) return;
      this.runAction = action;

      try {
        if (action === 'pause') {
          await OpenFangAPI.v1.runs.pause(this.selectedRunId);
          OpenFangToast.success('Run paused.');
        } else if (action === 'resume') {
          await OpenFangAPI.v1.runs.resume(this.selectedRunId);
          OpenFangToast.success('Run resumed.');
        } else if (action === 'cancel') {
          await OpenFangAPI.v1.runs.cancel(this.selectedRunId);
          OpenFangToast.success('Run cancelled.');
        }

        await this.loadRuns({ silent: true, refreshDetail: true });
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to ' + action + ' run.');
      }

      this.runAction = '';
    },

    confirmCancelRun() {
      var self = this;
      OpenFangUtils.confirmAction(
        'Cancel Run',
        'Cancel this workflow run? This stops the run and cannot be resumed.',
        function() {
          self.executeRunAction('cancel');
        }
      );
    },

    dispatchCanRetry(dispatch) {
      return !!(dispatch && (dispatch.status === 'failed' || dispatch.status === 'cancelled'));
    },

    dispatchCanCancel(dispatch) {
      return !!(dispatch && (
        dispatch.status === 'pending'
        || dispatch.status === 'running'
        || dispatch.status === 'waiting_hitl'
      ));
    },

    async retryDispatch(dispatch) {
      if (!dispatch || !this.dispatchCanRetry(dispatch)) return;
      this.dispatchActionId = 'retry:' + dispatch.id;

      try {
        await OpenFangAPI.v1.dispatches.retry(dispatch.id);
        OpenFangToast.success('Dispatch retry requested.');
        await this.loadRuns({ silent: true, refreshDetail: true });
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to retry dispatch.');
      }

      this.dispatchActionId = '';
    },

    confirmCancelDispatch(dispatch) {
      if (!dispatch || !this.dispatchCanCancel(dispatch)) return;

      var self = this;
      OpenFangUtils.confirmAction(
        'Cancel Dispatch',
        'Cancel this dispatch? Any active execution for the target agent will be stopped.',
        function() {
          self.cancelDispatch(dispatch);
        }
      );
    },

    async cancelDispatch(dispatch) {
      if (!dispatch) return;
      this.dispatchActionId = 'cancel:' + dispatch.id;

      try {
        await OpenFangAPI.v1.dispatches.cancel(dispatch.id);
        OpenFangToast.success('Dispatch cancelled.');
        await this.loadRuns({ silent: true, refreshDetail: true });
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to cancel dispatch.');
      }

      this.dispatchActionId = '';
    },

    isTextHitl(kind) {
      return kind === 'clarification' || kind === 'freeform';
    },

    isApprovalHitl(kind) {
      return kind === 'approval';
    },

    isChoiceHitl(kind) {
      return kind === 'choice';
    },

    hitlKindLabel(kind) {
      if (!kind) return 'Unknown';
      if (kind === 'clarification') return 'Clarification';
      if (kind === 'freeform') return 'Freeform';
      if (kind === 'approval') return 'Approval';
      if (kind === 'choice') return 'Choice';
      return this.statusLabel(kind);
    },

    hitlKindBadgeClass(kind) {
      if (!kind) return 'badge badge-dim';
      return 'badge badge-hitl-kind-' + kind;
    },

    choicesFor(request) {
      if (!request || !request.context || typeof request.context !== 'object') {
        return [];
      }

      var rawChoices = [];
      if (Array.isArray(request.context.choices)) {
        rawChoices = request.context.choices;
      } else if (Array.isArray(request.context.options)) {
        rawChoices = request.context.options;
      } else if (Array.isArray(request.context.values)) {
        rawChoices = request.context.values;
      }

      return rawChoices.map(function(choice, index) {
        if (typeof choice === 'string') {
          return {
            value: choice,
            label: choice,
            description: ''
          };
        }

        if (!choice || typeof choice !== 'object') {
          return null;
        }

        var value = choice.value || choice.id || choice.key || choice.name || choice.label;
        if (value === undefined || value === null || value === '') {
          value = 'choice_' + index;
        }

        return {
          value: String(value),
          label: choice.label || choice.title || choice.text || choice.name || String(value),
          description: choice.description || choice.help || ''
        };
      }).filter(Boolean);
    },

    async submitTextHitl(request) {
      var value = (this.hitlDrafts[request.id] || '').trim();
      if (!value) {
        OpenFangToast.error('Enter a response before submitting.');
        return;
      }

      var submitted = await this.submitHitlResponse(request, {
        type: 'text',
        value: value
      });

      if (submitted) {
        this.hitlDrafts[request.id] = '';
      }
    },

    async submitApprovalHitl(request, decision) {
      await this.submitHitlResponse(request, {
        type: 'approval',
        value: decision
      });
    },

    async submitChoiceHitl(request, choice) {
      await this.submitHitlResponse(request, {
        type: 'choice',
        value: choice.value
      });
    },

    async submitHitlResponse(request, responsePayload) {
      if (!request) return false;
      this.hitlSubmittingId = request.id;
      var submitted = false;

      try {
        await OpenFangAPI.v1.hitl.answer(request.id, {
          response: responsePayload,
          metadata: {
            source: 'dashboard',
            request_kind: request.kind
          }
        });

        this.upsertHitlRequest(Object.assign({}, request, {
          status: 'answered',
          response: responsePayload,
          answered_at: new Date().toISOString()
        }));
        OpenFangToast.success('HITL response submitted.');
        submitted = true;
        await this.loadRuns({ silent: true, refreshDetail: true });
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to submit HITL response.');
      }

      this.hitlSubmittingId = '';
      return submitted;
    },

    setTab(tab) {
      this.selectedTab = tab;
      if (tab === 'events') {
        this.scrollEventLogToBottom();
      }
    },

    connectEventStream(runId) {
      this.closeEventStream();
      this.eventEntries = [];
      this.eventConnectionState = 'connecting';

      var self = this;
      this.eventStream = OpenFangSSE.connect('/api/v1/runs/' + runId + '/events', {
        'stream.snapshot': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.applyStreamSnapshot(data);
          self.appendEventEntry('stream.snapshot', data, event);
        },
        'stream.reset': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.appendEventEntry('stream.reset', data, event);
        },
        'run.updated': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.applyRunUpdate(data);
          self.appendEventEntry('run.updated', data, event);
        },
        'dispatch.updated': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.upsertDispatch(data);
          self.appendEventEntry('dispatch.updated', data, event);
        },
        'hitl.requested': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.upsertHitlRequest(data);
          self.appendEventEntry('hitl.requested', data, event);
        },
        'hitl.answered': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.upsertHitlRequest(data);
          self.appendEventEntry('hitl.answered', data, event);
        },
        'hitl.cancelled': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.upsertHitlRequest(data);
          self.appendEventEntry('hitl.cancelled', data, event);
        },
        'hitl.timed_out': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.upsertHitlRequest(data);
          self.appendEventEntry('hitl.timed_out', data, event);
        },
        'keepalive': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.appendEventEntry('keepalive', data, event);
        }
      }, { reconnect: true });
    },

    closeEventStream() {
      if (this.eventStream) {
        this.eventStream.close();
        this.eventStream = null;
      }
    },

    applyStreamSnapshot(snapshot) {
      if (!snapshot) return;
      if (snapshot.run) {
        this.applyRunUpdate(snapshot.run);
      }
      if (snapshot.dispatches) {
        this.dispatches = this.sortDispatches(snapshot.dispatches);
      }
      if (snapshot.hitl_requests) {
        this.hitlRequests = this.sortHitlRequests(snapshot.hitl_requests);
      }
    },

    applyRunUpdate(partial) {
      if (!partial || !partial.id) return;

      if (this.selectedRun && this.selectedRun.id === partial.id) {
        this.selectedRun = Object.assign({}, this.selectedRun, partial);
      } else if (!this.selectedRun && this.selectedRunId === partial.id) {
        this.selectedRun = partial;
      }

      this.mergeRunSummary(partial);
    },

    mergeRunSummary(partial) {
      if (!partial || !partial.id) return;

      var nextRuns = this.runs.slice();
      var updated = false;

      for (var i = 0; i < nextRuns.length; i++) {
        if (nextRuns[i].id !== partial.id) continue;
        nextRuns[i] = Object.assign({}, nextRuns[i], {
          workflow_id: partial.workflow_id || nextRuns[i].workflow_id,
          status: partial.status || nextRuns[i].status,
          current_step_id: partial.current_step_id === undefined
            ? nextRuns[i].current_step_id
            : partial.current_step_id,
          waiting_kind: partial.waiting_kind === undefined
            ? nextRuns[i].waiting_kind
            : partial.waiting_kind,
          updated_at: partial.updated_at || nextRuns[i].updated_at,
          started_at: partial.started_at || nextRuns[i].started_at
        });
        updated = true;
        break;
      }

      if (!updated && partial.workflow_id) {
        nextRuns.push({
          id: partial.id,
          workflow_id: partial.workflow_id,
          status: partial.status || 'running',
          current_step_id: partial.current_step_id || null,
          waiting_kind: partial.waiting_kind || null,
          started_at: partial.started_at || null,
          updated_at: partial.updated_at || null
        });
      }

      this.runs = this.sortRuns(nextRuns);
    },

    upsertSignal(signal) {
      if (!signal || !signal.id) return;

      var nextSignals = this.signals.slice();
      var replaced = false;

      for (var i = 0; i < nextSignals.length; i++) {
        if (nextSignals[i].id === signal.id) {
          nextSignals[i] = signal;
          replaced = true;
          break;
        }
      }

      if (!replaced) nextSignals.push(signal);
      this.signals = this.sortSignals(nextSignals);
    },

    upsertDispatch(dispatch) {
      if (!dispatch || !dispatch.id) return;

      var nextDispatches = this.dispatches.slice();
      var replaced = false;

      for (var i = 0; i < nextDispatches.length; i++) {
        if (nextDispatches[i].id === dispatch.id) {
          nextDispatches[i] = Object.assign({}, nextDispatches[i], dispatch);
          replaced = true;
          break;
        }
      }

      if (!replaced) nextDispatches.push(dispatch);
      this.dispatches = this.sortDispatches(nextDispatches);
    },

    upsertHitlRequest(request) {
      if (!request || !request.id) return;

      var nextRequests = this.hitlRequests.slice();
      var replaced = false;

      for (var i = 0; i < nextRequests.length; i++) {
        if (nextRequests[i].id === request.id) {
          nextRequests[i] = Object.assign({}, nextRequests[i], request);
          replaced = true;
          break;
        }
      }

      if (!replaced) nextRequests.push(request);
      this.hitlRequests = this.sortHitlRequests(nextRequests);
    },

    appendEventEntry(name, data, rawEvent) {
      var nextEntries = this.eventEntries.slice();
      nextEntries.push({
        key: (rawEvent && rawEvent.lastEventId ? rawEvent.lastEventId : String(Date.now()))
          + ':' + name + ':' + nextEntries.length,
        id: rawEvent && rawEvent.lastEventId ? rawEvent.lastEventId : '',
        name: name,
        received_at: new Date().toISOString(),
        data: data === undefined ? null : data
      });

      if (nextEntries.length > 200) {
        nextEntries = nextEntries.slice(nextEntries.length - 200);
      }

      this.eventEntries = nextEntries;
      if (this.selectedTab === 'events' && this.autoScrollEvents) {
        this.scrollEventLogToBottom();
      }
    },

    scrollEventLogToBottom() {
      var self = this;
      setTimeout(function() {
        if (self.$refs && self.$refs.eventLog) {
          self.$refs.eventLog.scrollTop = self.$refs.eventLog.scrollHeight;
        }
      }, 0);
    },

    eventConnectionBadgeClass() {
      if (this.eventConnectionState === 'connected') return 'badge badge-run-running';
      if (this.eventConnectionState === 'connecting') return 'badge badge-run-pending';
      return 'badge badge-run-cancelled';
    },

    toggleAutoScrollEvents() {
      this.autoScrollEvents = !this.autoScrollEvents;
      if (this.autoScrollEvents) {
        this.scrollEventLogToBottom();
      }
    },

    hasRecoveryCheckpoint(checkpoints) {
      for (var i = 0; i < checkpoints.length; i++) {
        if (checkpoints[i] && checkpoints[i].kind === 'run_recovered_needs_resume') {
          return true;
        }
      }
      return false;
    }
  };
}

function compareTimestampsDesc(left, right) {
  var leftTs = Date.parse(left || '');
  var rightTs = Date.parse(right || '');

  if (isNaN(leftTs) && isNaN(rightTs)) return 0;
  if (isNaN(leftTs)) return 1;
  if (isNaN(rightTs)) return -1;
  if (leftTs > rightTs) return -1;
  if (leftTs < rightTs) return 1;
  return 0;
}

function compareStrings(left, right) {
  var leftValue = left || '';
  var rightValue = right || '';
  if (leftValue < rightValue) return -1;
  if (leftValue > rightValue) return 1;
  return 0;
}
