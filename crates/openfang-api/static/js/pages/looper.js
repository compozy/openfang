// OpenFang Looper Runs Page — iterative task execution with live SSE progress
'use strict';

function looperPage() {
  return {
    looperRuns: [],
    tasks: [],
    loading: true,
    loadError: '',
    detailLoading: false,
    detailError: '',
    filterStatus: '',
    filterMode: '',
    selectedRunId: '',
    selectedRun: null,
    selectedTab: 'overview',
    subtasks: [],
    runAction: '',
    eventEntries: [],
    eventStream: null,
    eventConnectionState: 'idle',
    autoScrollEvents: true,
    clockTimer: null,
    nowTick: Date.now(),
    listRequestToken: 0,
    detailRequestToken: 0,
    showCreateForm: false,
    createForm: {
      task_id: '',
      execution_mode: 'sequential',
      max_parallelism: 4,
      selection_strategy: 'first'
    },
    createSubmitting: false,

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

    get statusOptions() {
      return [
        { value: '', label: 'All Statuses' },
        { value: 'pending', label: 'Pending' },
        { value: 'running', label: 'Running' },
        { value: 'paused', label: 'Paused' },
        { value: 'completed', label: 'Completed' },
        { value: 'failed', label: 'Failed' },
        { value: 'cancelled', label: 'Cancelled' }
      ];
    },

    get modeOptions() {
      return [
        { value: '', label: 'All Modes' },
        { value: 'sequential', label: 'Sequential' },
        { value: 'parallel', label: 'Parallel' }
      ];
    },

    get filteredRuns() {
      var self = this;
      return this.looperRuns.filter(function(run) {
        if (!run) return false;
        if (self.filterStatus && run.status !== self.filterStatus) return false;
        if (self.filterMode && run.execution_mode !== self.filterMode) return false;
        return true;
      });
    },

    normalizeList(payload) {
      if (Array.isArray(payload)) return payload;
      if (payload && Array.isArray(payload.items)) return payload.items;
      return [];
    },

    sortRuns(items) {
      return (items || []).slice().sort(function(left, right) {
        return compareTimestampsDesc(left && left.updated_at, right && right.updated_at)
          || compareStrings(left && left.id, right && right.id);
      });
    },

    sortSubtasks(items) {
      return (items || []).slice().sort(function(left, right) {
        var orderA = left && left.order !== undefined ? left.order : 9999;
        var orderB = right && right.order !== undefined ? right.order : 9999;
        if (orderA !== orderB) return orderA - orderB;
        return compareTimestampsDesc(left && left.updated_at, right && right.updated_at)
          || compareStrings(left && left.id, right && right.id);
      });
    },

    async loadData() {
      this.loading = true;
      this.loadError = '';

      await Promise.all([
        this.loadTasks(),
        this.loadRuns({ refreshDetail: true, silent: true })
      ]);

      this.loading = false;
    },

    async loadTasks() {
      try {
        var response = await OpenFangAPI.v1.tasks.list();
        this.tasks = this.normalizeList(response);
      } catch (e) {
        this.tasks = [];
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
        var response = await OpenFangAPI.v1.looper.list({ limit: 200 });
        if (requestToken !== this.listRequestToken) return;

        this.looperRuns = this.sortRuns(this.normalizeList(response));

        var nextSelectedId = this.resolveSelectedRunId(this.filteredRuns);
        if (!nextSelectedId) {
          this.clearSelection();
        } else if (nextSelectedId !== this.selectedRunId) {
          await this.selectRun(nextSelectedId, false);
        } else if (options.refreshDetail !== false && this.selectedRunId) {
          await this.refreshSelectedRun();
        }
      } catch (e) {
        if (requestToken !== this.listRequestToken) return;
        this.looperRuns = [];
        this.loadError = e.message || 'Could not load looper runs.';
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
      this.subtasks = [];
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
          OpenFangAPI.v1.looper.get(runId),
          OpenFangAPI.v1.looper.subtasks(runId)
        ]);
        if (requestToken !== this.detailRequestToken || this.selectedRunId !== runId) return;

        this.selectedRun = responses[0];
        this.subtasks = this.sortSubtasks(this.normalizeList(responses[1]));
        this.mergeRunSummary(this.selectedRun);
      } catch (e) {
        if (requestToken !== this.detailRequestToken || this.selectedRunId !== runId) return;
        this.detailError = e.message || 'Could not load looper run details.';
      }

      if (requestToken === this.detailRequestToken && this.selectedRunId === runId) {
        this.detailLoading = false;
      }
    },

    clearSelection() {
      this.selectedRunId = '';
      this.selectedRun = null;
      this.detailError = '';
      this.detailLoading = false;
      this.selectedTab = 'overview';
      this.subtasks = [];
      this.eventEntries = [];
      this.eventConnectionState = 'idle';
      this.closeEventStream();
    },

    // ── Progress helpers ──

    progressPercent(run) {
      if (!run) return 0;
      var total = run.total_subtasks || 0;
      if (!total) return run.status === 'completed' ? 100 : 0;
      if (run.status === 'completed') return 100;
      var completed = (run.completed_subtasks || 0) + (run.failed_subtasks || 0);
      return Math.min(100, Math.max(0, Math.round((completed / total) * 100)));
    },

    progressLabel(run) {
      if (!run) return '';
      var total = run.total_subtasks || 0;
      var completed = run.completed_subtasks || 0;
      var failed = run.failed_subtasks || 0;
      if (!total) return 'No subtasks';
      var parts = completed + ' / ' + total + ' done';
      if (failed > 0) parts += ', ' + failed + ' failed';
      return parts;
    },

    progressBarSegments(run) {
      if (!run) return { completed: 0, failed: 0 };
      var total = run.total_subtasks || 1;
      return {
        completed: Math.round(((run.completed_subtasks || 0) / total) * 100),
        failed: Math.round(((run.failed_subtasks || 0) / total) * 100)
      };
    },

    // ── Status & badge helpers ──

    statusBadgeClass(status) {
      return OpenFangUtils.statusBadge(status);
    },

    statusLabel(status) {
      if (!status) return 'Unknown';
      return String(status)
        .replace(/_/g, ' ')
        .replace(/\b\w/g, function(letter) { return letter.toUpperCase(); });
    },

    modeBadgeClass(mode) {
      if (mode === 'parallel') return 'badge badge-run-running';
      return 'badge badge-run-pending';
    },

    modeLabel(mode) {
      if (!mode) return '';
      return mode === 'parallel' ? 'Parallel' : 'Sequential';
    },

    strategyLabel(strategy) {
      if (!strategy) return '-';
      return String(strategy)
        .replace(/_/g, ' ')
        .replace(/\b\w/g, function(letter) { return letter.toUpperCase(); });
    },

    subtaskStatusIcon(status) {
      if (status === 'completed' || status === 'done') return '\u2713';
      if (status === 'running') return '\u25B6';
      if (status === 'failed') return '\u2717';
      return '\u25CB';
    },

    subtaskStatusClass(status) {
      if (status === 'completed' || status === 'done') return 'looper-subtask-done';
      if (status === 'running') return 'looper-subtask-running';
      if (status === 'failed') return 'looper-subtask-failed';
      return 'looper-subtask-pending';
    },

    taskName(taskId) {
      if (!taskId) return 'Unknown';
      for (var i = 0; i < this.tasks.length; i++) {
        if (this.tasks[i].id === taskId) {
          return this.tasks[i].title || this.tasks[i].name || taskId;
        }
      }
      return taskId;
    },

    // ── Time helpers ──

    formatDateTime(value) {
      return OpenFangUtils.formatDateTime(value);
    },

    relativeTime(value) {
      var tick = this.nowTick;
      if (tick < 0) return '';
      return OpenFangUtils.timeAgo(value);
    },

    // ── Actions ──

    canPause(run) {
      return !!(run && run.status === 'running');
    },

    canResume(run) {
      return !!(run && run.status === 'paused');
    },

    canCancel(run) {
      return !!(run && (
        run.status === 'pending'
        || run.status === 'running'
        || run.status === 'paused'
      ));
    },

    async executeRunAction(action) {
      if (!this.selectedRunId || !this.selectedRun) return;
      this.runAction = action;

      try {
        if (action === 'pause') {
          await OpenFangAPI.v1.looper.pause(this.selectedRunId);
          OpenFangToast.success('Looper run paused.');
        } else if (action === 'resume') {
          await OpenFangAPI.v1.looper.resume(this.selectedRunId);
          OpenFangToast.success('Looper run resumed.');
        } else if (action === 'cancel') {
          await OpenFangAPI.v1.looper.cancel(this.selectedRunId);
          OpenFangToast.success('Looper run cancelled.');
        }

        await this.loadRuns({ silent: true, refreshDetail: true });
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to ' + action + ' looper run.');
      }

      this.runAction = '';
    },

    confirmCancelRun() {
      var self = this;
      OpenFangUtils.confirmAction(
        'Cancel Looper Run',
        'Cancel this looper run? All pending subtasks will be abandoned.',
        function() {
          self.executeRunAction('cancel');
        }
      );
    },

    // ── Create form ──

    openCreateForm() {
      this.showCreateForm = true;
      this.createForm = {
        task_id: '',
        execution_mode: 'sequential',
        max_parallelism: 4,
        selection_strategy: 'first'
      };
    },

    closeCreateForm() {
      this.showCreateForm = false;
    },

    async submitCreateForm() {
      if (!this.createForm.task_id) {
        OpenFangToast.error('Please select a task.');
        return;
      }

      this.createSubmitting = true;

      try {
        var body = {
          task_id: this.createForm.task_id,
          execution_mode: this.createForm.execution_mode,
          selection_strategy: this.createForm.selection_strategy
        };

        if (this.createForm.execution_mode === 'parallel') {
          body.max_parallelism = parseInt(this.createForm.max_parallelism, 10) || 4;
        }

        await OpenFangAPI.v1.looper.create(body);
        OpenFangToast.success('Looper run created.');
        this.showCreateForm = false;
        await this.loadRuns({ refreshDetail: true });
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to create looper run.');
      }

      this.createSubmitting = false;
    },

    // ── SSE streaming ──

    connectEventStream(runId) {
      this.closeEventStream();
      this.eventEntries = [];
      this.eventConnectionState = 'connecting';

      var self = this;
      this.eventStream = OpenFangSSE.connect('/api/v1/looper-runs/' + runId + '/events', {
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
        'subtask.started': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.upsertSubtask(data);
          self.appendEventEntry('subtask.started', data, event);
        },
        'subtask.completed': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.upsertSubtask(data);
          self.appendEventEntry('subtask.completed', data, event);
        },
        'subtask.failed': function(data, event) {
          if (self.selectedRunId !== runId) return;
          self.eventConnectionState = 'connected';
          self.upsertSubtask(data);
          self.appendEventEntry('subtask.failed', data, event);
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
      if (snapshot.subtasks) {
        this.subtasks = this.sortSubtasks(snapshot.subtasks);
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

      var nextRuns = this.looperRuns.slice();
      var updated = false;

      for (var i = 0; i < nextRuns.length; i++) {
        if (nextRuns[i].id !== partial.id) continue;
        nextRuns[i] = Object.assign({}, nextRuns[i], {
          status: partial.status || nextRuns[i].status,
          total_subtasks: partial.total_subtasks !== undefined
            ? partial.total_subtasks : nextRuns[i].total_subtasks,
          completed_subtasks: partial.completed_subtasks !== undefined
            ? partial.completed_subtasks : nextRuns[i].completed_subtasks,
          failed_subtasks: partial.failed_subtasks !== undefined
            ? partial.failed_subtasks : nextRuns[i].failed_subtasks,
          updated_at: partial.updated_at || nextRuns[i].updated_at
        });
        updated = true;
        break;
      }

      if (!updated && partial.task_id) {
        nextRuns.push(partial);
      }

      this.looperRuns = this.sortRuns(nextRuns);
    },

    upsertSubtask(subtask) {
      if (!subtask || !subtask.id) return;

      var nextSubtasks = this.subtasks.slice();
      var replaced = false;

      for (var i = 0; i < nextSubtasks.length; i++) {
        if (nextSubtasks[i].id === subtask.id) {
          nextSubtasks[i] = Object.assign({}, nextSubtasks[i], subtask);
          replaced = true;
          break;
        }
      }

      if (!replaced) nextSubtasks.push(subtask);
      this.subtasks = this.sortSubtasks(nextSubtasks);

      // Update counters on selectedRun based on subtask status changes
      if (this.selectedRun) {
        this.recalcRunCounters();
      }
    },

    recalcRunCounters() {
      if (!this.selectedRun) return;
      var completed = 0;
      var failed = 0;
      for (var i = 0; i < this.subtasks.length; i++) {
        var s = this.subtasks[i].status;
        if (s === 'completed' || s === 'done') completed++;
        else if (s === 'failed') failed++;
      }
      this.selectedRun = Object.assign({}, this.selectedRun, {
        completed_subtasks: completed,
        failed_subtasks: failed,
        total_subtasks: this.subtasks.length || this.selectedRun.total_subtasks
      });
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

    setTab(tab) {
      this.selectedTab = tab;
      if (tab === 'events') {
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

    formatJson(value) {
      if (value === undefined || value === null) return '{}';
      try {
        return JSON.stringify(value, null, 2);
      } catch (e) {
        return String(value);
      }
    }
  };
}
