// OpenFang Dispatches Page — durable agent dispatch visibility with lineage and live SSE
'use strict';

function dispatchesPage() {
  return {
    dispatches: [],
    loading: true,
    loadError: '',
    detailLoading: false,
    detailError: '',
    filterStatus: '',
    filterKind: '',
    selectedDispatchId: '',
    selectedDispatch: null,
    selectedChildren: [],
    relatedHitlRequests: [],
    lineageTree: [],
    lineageRows: [],
    dispatchDetails: {},
    listRequestToken: 0,
    detailRequestToken: 0,
    treeRequestToken: 0,
    eventStream: null,
    eventEntries: [],
    eventConnectionState: 'idle',
    autoScrollEvents: true,
    dispatchActionId: '',
    clockTimer: null,
    nowTick: Date.now(),

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
        { value: 'waiting_hitl', label: 'Waiting HITL' },
        { value: 'completed', label: 'Completed' },
        { value: 'failed', label: 'Failed' },
        { value: 'cancelled', label: 'Cancelled' }
      ];
    },

    get kindOptions() {
      return [
        { value: '', label: 'All Kinds' },
        { value: 'call', label: 'Call' },
        { value: 'send', label: 'Send' },
        { value: 'spawn', label: 'Spawn' }
      ];
    },

    get selectedParent() {
      if (!this.selectedDispatch || !this.selectedDispatch.parent_dispatch_id) return null;
      return this.dispatchDetails[this.selectedDispatch.parent_dispatch_id] || null;
    },

    normalizeList(payload) {
      if (Array.isArray(payload)) return payload;
      if (payload && Array.isArray(payload.items)) return payload.items;
      return [];
    },

    hasDispatchDetail(dispatch) {
      if (!dispatch) return false;
      return dispatch.attempt !== undefined
        || dispatch.started_at !== undefined
        || dispatch.completed_at !== undefined
        || dispatch.parent_dispatch_id !== undefined
        || dispatch.spawned_agent_id !== undefined
        || dispatch.input !== undefined
        || dispatch.result !== undefined
        || dispatch.error !== undefined;
    },

    dispatchQueryParams() {
      return {
        limit: 200,
        status: this.filterStatus || undefined,
        kind: this.filterKind || undefined
      };
    },

    applyClientFilters(items) {
      return (items || []).filter(function(dispatch) {
        if (this.filterStatus && dispatch.status !== this.filterStatus) {
          return false;
        }

        if (this.filterKind && dispatch.kind !== this.filterKind) {
          return false;
        }

        return true;
      }, this);
    },

    async loadData() {
      await this.loadDispatches({ refreshDetail: true });
    },

    async loadDispatches(options) {
      options = options || {};
      var requestToken = ++this.listRequestToken;

      if (!options.silent) {
        this.loading = true;
      }

      this.loadError = '';

      try {
        var response = await OpenFangAPI.v1.dispatches.list(this.dispatchQueryParams());
        if (requestToken !== this.listRequestToken) return;

        var items = this.sortDispatches(this.applyClientFilters(this.normalizeList(response).map(
          function(dispatch) {
            return this.mergeWithCachedDispatch(dispatch);
          },
          this
        )));
        this.dispatches = items;

        var nextSelectedDispatchId = this.resolveSelectedDispatchId(items);
        if (!nextSelectedDispatchId) {
          this.clearSelection();
        } else if (nextSelectedDispatchId !== this.selectedDispatchId) {
          await this.selectDispatch(nextSelectedDispatchId);
        } else if (options.refreshDetail !== false && this.selectedDispatchId) {
          await this.refreshSelectedDispatch();
        }

        this.hydrateDispatchSummaries(items, requestToken);
      } catch (e) {
        if (requestToken !== this.listRequestToken) return;
        this.dispatches = [];
        this.loadError = e.message || 'Could not load dispatches.';
        this.clearSelection();
      }

      if (!options.silent) {
        this.loading = false;
      }
    },

    async hydrateDispatchSummaries(items, requestToken) {
      var self = this;
      var detailPromises = (items || []).map(async function(dispatch) {
        try {
          var detail = await self.getDispatchDetail(dispatch.id);
          if (requestToken !== self.listRequestToken) return;
          self.cacheDispatch(detail);
        } catch (e) {
          // Keep the summary record if detail hydration fails.
        }
      });

      await Promise.all(detailPromises);
    },

    resolveSelectedDispatchId(items) {
      if (!items.length) return '';

      for (var i = 0; i < items.length; i++) {
        if (items[i].id === this.selectedDispatchId) {
          return this.selectedDispatchId;
        }
      }

      return items[0].id;
    },

    clearSelection() {
      this.selectedDispatchId = '';
      this.selectedDispatch = null;
      this.selectedChildren = [];
      this.relatedHitlRequests = [];
      this.lineageTree = [];
      this.lineageRows = [];
      this.detailError = '';
      this.detailLoading = false;
      this.eventEntries = [];
      this.eventConnectionState = 'idle';
      this.closeEventStream();
    },

    async selectDispatch(dispatchId) {
      if (!dispatchId) {
        this.clearSelection();
        return;
      }

      if (dispatchId === this.selectedDispatchId && this.selectedDispatch) {
        return;
      }

      this.selectedDispatchId = dispatchId;
      this.selectedDispatch = this.mergeWithCachedDispatch({ id: dispatchId });
      this.detailError = '';
      this.detailLoading = true;
      this.selectedChildren = [];
      this.relatedHitlRequests = [];
      this.lineageTree = [];
      this.lineageRows = [];
      this.connectEventStream(dispatchId);
      await this.refreshSelectedDispatch();
    },

    async refreshSelectedDispatch(options) {
      options = options || {};
      var dispatchId = this.selectedDispatchId;
      if (!dispatchId) return;

      var requestToken = ++this.detailRequestToken;
      if (!options.silent) {
        this.detailLoading = true;
      }
      this.detailError = '';

      try {
        var responses = await Promise.all([
          this.getDispatchDetail(dispatchId, !!options.force),
          this.getDispatchChildren(dispatchId, true),
          OpenFangAPI.v1.hitl.list({ dispatch_id: dispatchId })
        ]);

        if (requestToken !== this.detailRequestToken || this.selectedDispatchId !== dispatchId) {
          return;
        }

        this.selectedDispatch = responses[0];
        this.selectedChildren = this.sortLineageChildren(this.normalizeList(responses[1]).map(
          function(child) {
            return this.mergeWithCachedDispatch(child);
          },
          this
        ));
        this.relatedHitlRequests = this.sortHitlRequests(this.normalizeList(responses[2]));
        await this.loadLineageTree(dispatchId);
      } catch (e) {
        if (requestToken !== this.detailRequestToken || this.selectedDispatchId !== dispatchId) {
          return;
        }
        this.detailError = e.message || 'Could not load dispatch detail.';
      }

      if (requestToken === this.detailRequestToken && this.selectedDispatchId === dispatchId) {
        this.detailLoading = false;
      }
    },

    async getDispatchDetail(id, force) {
      if (!id) return null;

      var cached = this.dispatchDetails[id];
      if (!force && this.hasDispatchDetail(cached)) {
        return cached;
      }

      var detail = await OpenFangAPI.v1.dispatches.get(id);
      return this.cacheDispatch(detail);
    },

    async getDispatchChildren(id, force) {
      if (!id) return [];
      var children = await OpenFangAPI.v1.dispatches.children(id);
      var childList = this.normalizeList(children);
      var nextChildren = [];

      for (var i = 0; i < childList.length; i++) {
        var child = childList[i];
        var enriched = force || !this.hasDispatchDetail(this.dispatchDetails[child.id])
          ? await this.getDispatchDetail(child.id)
          : this.mergeWithCachedDispatch(child);
        nextChildren.push(enriched || child);
      }

      return nextChildren;
    },

    mergeWithCachedDispatch(dispatch) {
      if (!dispatch || !dispatch.id) return dispatch;
      var cached = this.dispatchDetails[dispatch.id];
      if (!cached) return dispatch;
      return Object.assign({}, dispatch, cached);
    },

    cacheDispatch(dispatch) {
      if (!dispatch || !dispatch.id) return dispatch;

      var merged = Object.assign({}, this.dispatchDetails[dispatch.id] || {}, dispatch);
      this.dispatchDetails[dispatch.id] = merged;
      this.dispatchDetails = Object.assign({}, this.dispatchDetails);
      this.mergeDispatchIntoList(merged);

      if (this.selectedDispatch && this.selectedDispatch.id === merged.id) {
        this.selectedDispatch = Object.assign({}, this.selectedDispatch, merged);
      } else if (!this.selectedDispatch && this.selectedDispatchId === merged.id) {
        this.selectedDispatch = merged;
      }

      return merged;
    },

    mergeDispatchIntoList(dispatch) {
      if (!dispatch || !dispatch.id) return;

      var nextDispatches = this.dispatches.slice();
      var replaced = false;

      for (var i = 0; i < nextDispatches.length; i++) {
        if (nextDispatches[i].id !== dispatch.id) continue;
        nextDispatches[i] = Object.assign({}, nextDispatches[i], dispatch);
        replaced = true;
        break;
      }

      if (!replaced) return;
      this.dispatches = this.sortDispatches(nextDispatches);
    },

    sortDispatches(items) {
      return (items || []).slice().sort(function(left, right) {
        return compareDispatchTimestampsDesc(
          left && (left.updated_at || left.started_at || left.completed_at),
          right && (right.updated_at || right.started_at || right.completed_at)
        ) || compareDispatchStrings(left && left.id, right && right.id);
      });
    },

    sortLineageChildren(items) {
      return (items || []).slice().sort(function(left, right) {
        var leftPrimary = left && (left.started_at || left.updated_at || left.completed_at);
        var rightPrimary = right && (right.started_at || right.updated_at || right.completed_at);
        return compareDispatchTimestampsAsc(leftPrimary, rightPrimary)
          || compareDispatchStrings(left && left.id, right && right.id);
      });
    },

    sortHitlRequests(items) {
      return (items || []).slice().sort(function(left, right) {
        var sequenceOrder = (left.sequence_no || 0) - (right.sequence_no || 0);
        if (sequenceOrder !== 0) return sequenceOrder;
        return compareDispatchTimestampsAsc(left && left.created_at, right && right.created_at);
      });
    },

    async loadLineageTree(dispatchId) {
      var requestToken = ++this.treeRequestToken;
      if (!dispatchId) {
        this.lineageTree = [];
        this.lineageRows = [];
        return;
      }

      try {
        var selectedDispatch = await this.getDispatchDetail(dispatchId);
        var rootDispatch = await this.resolveLineageRoot(selectedDispatch);
        var lineageNode = await this.buildLineageNode(rootDispatch, {});

        if (requestToken !== this.treeRequestToken || this.selectedDispatchId !== dispatchId) {
          return;
        }

        this.lineageTree = lineageNode ? [lineageNode] : [];
        this.lineageRows = lineageNode ? this.flattenLineageNode(lineageNode, 0, []) : [];
      } catch (e) {
        if (requestToken !== this.treeRequestToken || this.selectedDispatchId !== dispatchId) {
          return;
        }
        this.lineageTree = [];
        this.lineageRows = [];
      }
    },

    async resolveLineageRoot(dispatch) {
      var current = dispatch;
      var visited = {};

      while (current && current.parent_dispatch_id && !visited[current.id]) {
        visited[current.id] = true;
        current = await this.getDispatchDetail(current.parent_dispatch_id);
      }

      return current || dispatch;
    },

    async buildLineageNode(dispatch, visited) {
      if (!dispatch || !dispatch.id) return null;
      if (visited[dispatch.id]) {
        return {
          dispatch: dispatch,
          children: [],
          cycleDetected: true
        };
      }

      visited[dispatch.id] = true;
      var children = await this.getDispatchChildren(dispatch.id, true);
      var nodes = [];

      for (var i = 0; i < children.length; i++) {
        var childDispatch = this.mergeWithCachedDispatch(children[i]);
        var childNode = await this.buildLineageNode(childDispatch, Object.assign({}, visited));
        if (childNode) {
          nodes.push(childNode);
        }
      }

      return {
        dispatch: dispatch,
        children: nodes
      };
    },

    flattenLineageNode(node, depth, rows) {
      var branchPrefix = '';
      if (depth === 0) {
        branchPrefix = '●';
      } else {
        branchPrefix = new Array(depth + 1).join('│ ') + '↳';
      }

      rows.push({
        key: node.dispatch.id + ':' + depth,
        depth: depth,
        branchPrefix: branchPrefix,
        dispatch: node.dispatch,
        isSelected: node.dispatch.id === this.selectedDispatchId,
        childCount: node.children.length,
        cycleDetected: !!node.cycleDetected
      });

      for (var i = 0; i < node.children.length; i++) {
        this.flattenLineageNode(node.children[i], depth + 1, rows);
      }

      return rows;
    },

    connectEventStream(dispatchId) {
      this.closeEventStream();
      this.eventEntries = [];
      this.eventConnectionState = 'connecting';

      var self = this;
      this.eventStream = OpenFangSSE.connect('/api/v1/dispatches/' + dispatchId + '/events', {
        'stream.snapshot': function(data, event) {
          if (self.selectedDispatchId !== dispatchId) return;
          self.eventConnectionState = 'connected';
          self.applyDispatchSnapshot(data);
          self.appendEventEntry('stream.snapshot', data, event);
        },
        'stream.reset': function(data, event) {
          if (self.selectedDispatchId !== dispatchId) return;
          self.eventConnectionState = 'connected';
          self.appendEventEntry('stream.reset', data, event);
          self.refreshSelectedDispatch({ silent: true, force: true });
        },
        'dispatch.updated': function(data, event) {
          if (self.selectedDispatchId !== dispatchId) return;
          self.eventConnectionState = 'connected';
          self.applyDispatchUpdate(data);
          self.appendEventEntry('dispatch.updated', data, event);
        },
        'hitl.requested': function(data, event) {
          if (self.selectedDispatchId !== dispatchId) return;
          self.eventConnectionState = 'connected';
          self.upsertHitlRequest(data);
          self.appendEventEntry('hitl.requested', data, event);
        },
        'hitl.answered': function(data, event) {
          if (self.selectedDispatchId !== dispatchId) return;
          self.eventConnectionState = 'connected';
          self.upsertHitlRequest(data);
          self.appendEventEntry('hitl.answered', data, event);
        },
        'hitl.cancelled': function(data, event) {
          if (self.selectedDispatchId !== dispatchId) return;
          self.eventConnectionState = 'connected';
          self.upsertHitlRequest(data);
          self.appendEventEntry('hitl.cancelled', data, event);
        },
        'hitl.timed_out': function(data, event) {
          if (self.selectedDispatchId !== dispatchId) return;
          self.eventConnectionState = 'connected';
          self.upsertHitlRequest(data);
          self.appendEventEntry('hitl.timed_out', data, event);
        },
        'keepalive': function(data, event) {
          if (self.selectedDispatchId !== dispatchId) return;
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

    applyDispatchSnapshot(snapshot) {
      if (!snapshot) return;

      if (snapshot.dispatch) {
        this.applyDispatchUpdate(snapshot.dispatch);
      }

      if (snapshot.hitl_requests) {
        this.relatedHitlRequests = this.sortHitlRequests(snapshot.hitl_requests);
      }
    },

    applyDispatchUpdate(dispatch) {
      if (!dispatch || !dispatch.id) return;

      var merged = this.cacheDispatch(dispatch);
      if (this.selectedDispatchId === merged.id) {
        this.selectedDispatch = merged;
      }

      if (this.selectedDispatch && this.selectedDispatch.parent_dispatch_id) {
        this.loadLineageTree(this.selectedDispatchId);
      }
    },

    upsertHitlRequest(request) {
      if (!request || !request.id) return;

      var nextRequests = this.relatedHitlRequests.slice();
      var replaced = false;

      for (var i = 0; i < nextRequests.length; i++) {
        if (nextRequests[i].id !== request.id) continue;
        nextRequests[i] = Object.assign({}, nextRequests[i], request);
        replaced = true;
        break;
      }

      if (!replaced) {
        nextRequests.push(request);
      }

      this.relatedHitlRequests = this.sortHitlRequests(nextRequests);
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
      if (this.autoScrollEvents) {
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

    confirmRetryDispatch(dispatch) {
      if (!dispatch || !this.dispatchCanRetry(dispatch)) return;

      var self = this;
      OpenFangUtils.confirmAction(
        'Retry Dispatch',
        'Retry this dispatch? The target agent execution will be scheduled again.',
        function() {
          self.retryDispatch(dispatch);
        }
      );
    },

    async retryDispatch(dispatch) {
      if (!dispatch || !this.dispatchCanRetry(dispatch)) return;
      this.dispatchActionId = 'retry:' + dispatch.id;

      try {
        await OpenFangAPI.v1.dispatches.retry(dispatch.id);
        OpenFangToast.success('Dispatch retry requested.');
        await this.loadDispatches({ silent: true, refreshDetail: true });
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
      if (!dispatch || !this.dispatchCanCancel(dispatch)) return;
      this.dispatchActionId = 'cancel:' + dispatch.id;

      try {
        await OpenFangAPI.v1.dispatches.cancel(dispatch.id);
        OpenFangToast.success('Dispatch cancelled.');
        await this.loadDispatches({ silent: true, refreshDetail: true });
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to cancel dispatch.');
      }

      this.dispatchActionId = '';
    },

    selectParentDispatch() {
      if (!this.selectedDispatch || !this.selectedDispatch.parent_dispatch_id) return;
      this.selectDispatch(this.selectedDispatch.parent_dispatch_id);
    },

    statusBadgeClass(status) {
      return OpenFangUtils.statusBadge(status);
    },

    statusLabel(value) {
      if (!value) return 'Unknown';
      return String(value)
        .replace(/_/g, ' ')
        .replace(/\b\w/g, function(letter) { return letter.toUpperCase(); });
    },

    kindLabel(kind) {
      if (!kind) return 'Unknown';
      if (kind === 'call') return 'Call';
      if (kind === 'send') return 'Send';
      if (kind === 'spawn') return 'Spawn';
      return this.statusLabel(kind);
    },

    kindBadgeClass(kind) {
      if (!kind) return 'badge badge-dim';
      return 'badge badge-dispatch-kind-' + kind;
    },

    hitlKindLabel(kind) {
      if (!kind) return 'Unknown';
      return this.statusLabel(kind);
    },

    hitlKindBadgeClass(kind) {
      if (!kind) return 'badge badge-dim';
      return 'badge badge-hitl-kind-' + kind;
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

    attemptLabel(dispatch) {
      if (!dispatch || dispatch.attempt === undefined || dispatch.attempt === null) return '-';
      return String(dispatch.attempt);
    },

    childCountLabel(row) {
      if (!row || !row.childCount) return '';
      return row.childCount + ' child' + (row.childCount === 1 ? '' : 'ren');
    },

    copyDispatchId(dispatch) {
      if (!dispatch || !dispatch.id) return;
      OpenFangUtils.copyToClipboard(dispatch.id);
    }
  };
}

function compareDispatchTimestampsDesc(left, right) {
  var leftTs = Date.parse(left || '');
  var rightTs = Date.parse(right || '');

  if (isNaN(leftTs) && isNaN(rightTs)) return 0;
  if (isNaN(leftTs)) return 1;
  if (isNaN(rightTs)) return -1;
  if (leftTs > rightTs) return -1;
  if (leftTs < rightTs) return 1;
  return 0;
}

function compareDispatchTimestampsAsc(left, right) {
  return compareDispatchTimestampsDesc(right, left);
}

function compareDispatchStrings(left, right) {
  var leftValue = left || '';
  var rightValue = right || '';
  if (leftValue < rightValue) return -1;
  if (leftValue > rightValue) return 1;
  return 0;
}
