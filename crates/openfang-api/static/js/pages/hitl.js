// OpenFang HITL Inbox Page — durable request inbox with live SSE updates
'use strict';

function hitlPage() {
  return {
    requests: [],
    filterStatus: 'pending',
    selectedRequestId: '',
    loading: true,
    loadError: '',
    sseSource: null,
    clockTimer: null,
    nowTick: Date.now(),
    responseDrafts: {},
    submittingId: '',
    cancellingId: '',

    init() {
      var self = this;
      this.nowTick = Date.now();
      this.clockTimer = setInterval(function() {
        self.nowTick = Date.now();
      }, 30000);
      this.loadData();
      this.startSSE();
    },

    destroy() {
      if (this.clockTimer) {
        clearInterval(this.clockTimer);
        this.clockTimer = null;
      }
      if (this.sseSource) {
        this.sseSource.close();
        this.sseSource = null;
      }
    },

    get pendingCount() {
      return this.requests.filter(function(request) {
        return request.status === 'pending';
      }).length;
    },

    get filteredRequests() {
      var self = this;
      return this.sortRequests(this.requests).filter(function(request) {
        return request.status === self.filterStatus;
      });
    },

    get selectedRequest() {
      return this.findRequestById(this.selectedRequestId);
    },

    setFilterStatus(status) {
      this.filterStatus = status;
      this.ensureSelectedRequest();
    },

    findRequestById(id) {
      if (!id) return null;
      for (var i = 0; i < this.requests.length; i++) {
        if (this.requests[i].id === id) return this.requests[i];
      }
      return null;
    },

    ensureSelectedRequest() {
      var current = this.findRequestById(this.selectedRequestId);
      if (current && current.status === this.filterStatus) {
        return;
      }
      this.selectedRequestId = this.filteredRequests.length ? this.filteredRequests[0].id : '';
    },

    normalizeList(payload) {
      if (Array.isArray(payload)) return payload;
      if (payload && Array.isArray(payload.items)) return payload.items;
      return [];
    },

    sortRequests(items) {
      return (items || []).slice().sort(function(left, right) {
        var leftTs = Date.parse(left && left.created_at ? left.created_at : '');
        var rightTs = Date.parse(right && right.created_at ? right.created_at : '');

        if (isNaN(leftTs) && isNaN(rightTs)) {
          return (left.sequence_no || 0) - (right.sequence_no || 0);
        }
        if (isNaN(leftTs)) return 1;
        if (isNaN(rightTs)) return -1;
        if (leftTs !== rightTs) return leftTs - rightTs;
        return (left.sequence_no || 0) - (right.sequence_no || 0);
      });
    },

    replaceRequests(items) {
      this.requests = this.sortRequests(items);
      this.ensureSelectedRequest();
    },

    upsertRequest(nextRequest) {
      if (!nextRequest || !nextRequest.id) return;

      var nextItems = this.requests.slice();
      var replaced = false;

      for (var i = 0; i < nextItems.length; i++) {
        if (nextItems[i].id === nextRequest.id) {
          nextItems[i] = nextRequest;
          replaced = true;
          break;
        }
      }

      if (!replaced) {
        nextItems.push(nextRequest);
      }

      this.requests = this.sortRequests(nextItems);
      this.ensureSelectedRequest();
    },

    startSSE() {
      if (this.sseSource) {
        this.sseSource.close();
      }

      var self = this;
      this.sseSource = OpenFangSSE.connect('/api/v1/hitl-requests/stream', {
        'stream.snapshot': function(data) {
          self.replaceRequests(self.normalizeList(data));
        },
        'stream.reset': function() {
          self.replaceRequests([]);
        },
        'hitl.requested': function(data) {
          self.upsertRequest(data);
        },
        'hitl.answered': function(data) {
          self.upsertRequest(data);
        },
        'hitl.cancelled': function(data) {
          self.upsertRequest(data);
        },
        'hitl.timed_out': function(data) {
          self.upsertRequest(data);
        }
      }, { reconnect: true });
    },

    async loadData() {
      this.loading = true;
      this.loadError = '';

      try {
        var response = await OpenFangAPI.v1.hitl.list();
        this.replaceRequests(this.normalizeList(response));
      } catch (e) {
        this.loadError = e.message || 'Could not load HITL requests.';
      }

      this.loading = false;
    },

    selectRequest(requestId) {
      this.selectedRequestId = requestId;
    },

    isPending(request) {
      return request && request.status === 'pending';
    },

    isTextKind(kind) {
      return kind === 'clarification' || kind === 'freeform';
    },

    isApprovalKind(kind) {
      return kind === 'approval';
    },

    isChoiceKind(kind) {
      return kind === 'choice';
    },

    kindLabel(kind) {
      if (!kind) return 'Unknown';
      if (kind === 'clarification') return 'Clarification';
      if (kind === 'freeform') return 'Freeform';
      if (kind === 'approval') return 'Approval';
      if (kind === 'choice') return 'Choice';
      return kind;
    },

    kindBadgeClass(kind) {
      if (!kind) return 'badge badge-dim';
      return 'badge badge-hitl-kind-' + kind;
    },

    statusBadgeClass(status) {
      return OpenFangUtils.statusBadge(status);
    },

    statusLabel(status) {
      if (!status) return 'Unknown';
      if (status === 'pending') return 'Pending';
      if (status === 'answered') return 'Answered';
      if (status === 'cancelled') return 'Cancelled';
      if (status === 'timed_out') return 'Timed Out';
      return status.replace(/_/g, ' ');
    },

    responseSummary(request) {
      if (!request || !request.response) return '';
      var response = request.response;

      if (typeof response === 'string') return response;
      if (response && typeof response === 'object') {
        if (response.value !== undefined && response.value !== null) {
          return String(response.value);
        }
        try {
          return JSON.stringify(response);
        } catch (e) {
          return '';
        }
      }

      return String(response);
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

        var label = choice.label || choice.title || choice.text || choice.name || String(value);
        return {
          value: String(value),
          label: label,
          description: choice.description || choice.help || ''
        };
      }).filter(Boolean);
    },

    formatJson(value) {
      if (value === undefined || value === null) return '{}';
      try {
        return JSON.stringify(value, null, 2);
      } catch (e) {
        return String(value);
      }
    },

    timeWaiting(request) {
      var tick = this.nowTick;
      if (tick < 0) return '';
      if (!request || !request.created_at) return '';
      return OpenFangUtils.timeAgo(request.created_at);
    },

    answeredAt(request) {
      if (!request || !request.answered_at) return '';
      return OpenFangUtils.formatDateTime(request.answered_at);
    },

    createdAt(request) {
      if (!request || !request.created_at) return '';
      return OpenFangUtils.formatDateTime(request.created_at);
    },

    timeoutAt(request) {
      if (!request || !request.timeout_at) return '';
      return OpenFangUtils.formatDateTime(request.timeout_at);
    },

    async submitTextAnswer(request) {
      var value = (this.responseDrafts[request.id] || '').trim();
      if (!value) {
        OpenFangToast.error('Enter a response before submitting.');
        return;
      }

      var submitted = await this.submitAnswer(request, {
        type: 'text',
        value: value
      });
      if (submitted) {
        this.responseDrafts[request.id] = '';
      }
    },

    async submitApproval(request, decision) {
      await this.submitAnswer(request, {
        type: 'approval',
        value: decision
      });
    },

    async submitChoice(request, choice) {
      await this.submitAnswer(request, {
        type: 'choice',
        value: choice.value
      });
    },

    async submitAnswer(request, responsePayload) {
      this.submittingId = request.id;
      var submitted = false;

      try {
        await OpenFangAPI.v1.hitl.answer(request.id, {
          response: responsePayload,
          metadata: {
            source: 'dashboard',
            request_kind: request.kind
          }
        });

        this.upsertRequest({
          id: request.id,
          run_id: request.run_id,
          step_id: request.step_id,
          dispatch_id: request.dispatch_id,
          kind: request.kind,
          status: 'answered',
          question: request.question,
          context: request.context,
          response: responsePayload,
          sequence_no: request.sequence_no,
          created_at: request.created_at,
          answered_at: new Date().toISOString(),
          timeout_at: request.timeout_at
        });

        OpenFangToast.success('HITL response submitted.');
        submitted = true;
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to submit HITL response.');
      }

      this.submittingId = '';
      return submitted;
    },

    confirmCancel(request) {
      var self = this;
      OpenFangUtils.confirmAction(
        'Cancel HITL Request',
        'Cancel this request? The blocked workflow will stop waiting for human input.',
        function() {
          self.cancelRequest(request);
        }
      );
    },

    async cancelRequest(request) {
      this.cancellingId = request.id;

      try {
        await OpenFangAPI.v1.hitl.cancel(request.id);

        this.upsertRequest({
          id: request.id,
          run_id: request.run_id,
          step_id: request.step_id,
          dispatch_id: request.dispatch_id,
          kind: request.kind,
          status: 'cancelled',
          question: request.question,
          context: request.context,
          response: request.response || null,
          sequence_no: request.sequence_no,
          created_at: request.created_at,
          answered_at: request.answered_at,
          timeout_at: request.timeout_at
        });

        OpenFangToast.success('HITL request cancelled.');
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to cancel HITL request.');
      }

      this.cancellingId = '';
    }
  };
}
