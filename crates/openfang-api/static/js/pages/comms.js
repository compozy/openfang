// OpenFang Comms Page — Agent topology, inter-agent communication feed, A2A management
'use strict';

function commsPage() {
  return {
    topology: { nodes: [], edges: [] },
    events: [],
    loading: true,
    loadError: '',
    sseSource: null,
    showSendModal: false,
    showTaskModal: false,
    sendFrom: '',
    sendTo: '',
    sendMsg: '',
    sendLoading: false,
    taskTitle: '',
    taskDesc: '',
    taskAssign: '',
    taskLoading: false,

    // -- A2A state --
    a2aTab: 'topology',
    a2aAgents: [],
    a2aLoading: false,
    a2aLoadError: '',
    showDiscoverModal: false,
    discoverUrl: '',
    discoverLoading: false,
    showA2aSendModal: false,
    a2aSendAgent: '',
    a2aSendPayload: '{\n  "message": "Hello"\n}',
    a2aSendLoading: false,
    a2aTaskStatuses: {},
    a2aPollingTimers: {},

    async loadData() {
      this.loading = true;
      this.loadError = '';
      try {
        var results = await Promise.all([
          OpenFangAPI.get('/api/comms/topology'),
          OpenFangAPI.get('/api/comms/events?limit=200')
        ]);
        this.topology = results[0] || { nodes: [], edges: [] };
        this.events = results[1] || [];
        this.startSSE();
      } catch(e) {
        this.loadError = e.message || 'Could not load comms data.';
      }
      this.loading = false;
    },

    startSSE() {
      if (this.sseSource) this.sseSource.close();
      var self = this;
      this.sseSource = OpenFangSSE.connect('/api/comms/events/stream', {
        message: function(data) {
          if (!data) return;
          self.events.unshift(data);
          if (self.events.length > 200) self.events.length = 200;
          if (data.kind === 'agent_spawned' || data.kind === 'agent_terminated') {
            self.refreshTopology();
          }
        }
      });
    },

    stopSSE() {
      if (this.sseSource) {
        this.sseSource.close();
        this.sseSource = null;
      }
    },

    destroy() {
      this.stopSSE();
      this.stopAllA2aPolling();
    },

    async refreshTopology() {
      try {
        this.topology = await OpenFangAPI.get('/api/comms/topology');
      } catch(e) { /* silent */ }
    },

    rootNodes() {
      var childIds = {};
      this.topology.edges.forEach(function(e) {
        if (e.kind === 'parent_child') childIds[e.to] = true;
      });
      return this.topology.nodes.filter(function(n) { return !childIds[n.id]; });
    },

    childrenOf(id) {
      var childIds = {};
      this.topology.edges.forEach(function(e) {
        if (e.kind === 'parent_child' && e.from === id) childIds[e.to] = true;
      });
      return this.topology.nodes.filter(function(n) { return childIds[n.id]; });
    },

    peersOf(id) {
      var peerIds = {};
      this.topology.edges.forEach(function(e) {
        if (e.kind === 'peer') {
          if (e.from === id) peerIds[e.to] = true;
          if (e.to === id) peerIds[e.from] = true;
        }
      });
      return this.topology.nodes.filter(function(n) { return peerIds[n.id]; });
    },

    stateBadgeClass(state) {
      switch(state) {
        case 'Running': return 'badge badge-success';
        case 'Suspended': return 'badge badge-warning';
        case 'Terminated': case 'Crashed': return 'badge badge-danger';
        default: return 'badge badge-dim';
      }
    },

    eventBadgeClass(kind) {
      switch(kind) {
        case 'agent_message': return 'badge badge-info';
        case 'agent_spawned': return 'badge badge-success';
        case 'agent_terminated': return 'badge badge-danger';
        case 'task_posted': return 'badge badge-warning';
        case 'task_claimed': return 'badge badge-info';
        case 'task_completed': return 'badge badge-success';
        default: return 'badge badge-dim';
      }
    },

    eventIcon(kind) {
      switch(kind) {
        case 'agent_message': return '\u2709';
        case 'agent_spawned': return '+';
        case 'agent_terminated': return '\u2715';
        case 'task_posted': return '\u2691';
        case 'task_claimed': return '\u2690';
        case 'task_completed': return '\u2713';
        default: return '\u2022';
      }
    },

    eventLabel(kind) {
      switch(kind) {
        case 'agent_message': return 'Message';
        case 'agent_spawned': return 'Spawned';
        case 'agent_terminated': return 'Terminated';
        case 'task_posted': return 'Task Posted';
        case 'task_claimed': return 'Task Claimed';
        case 'task_completed': return 'Task Done';
        default: return kind;
      }
    },

    timeAgo(dateStr) {
      if (!dateStr) return '';
      var d = new Date(dateStr);
      var secs = Math.floor((Date.now() - d.getTime()) / 1000);
      if (secs < 60) return secs + 's ago';
      if (secs < 3600) return Math.floor(secs / 60) + 'm ago';
      if (secs < 86400) return Math.floor(secs / 3600) + 'h ago';
      return Math.floor(secs / 86400) + 'd ago';
    },

    openSendModal() {
      this.sendFrom = '';
      this.sendTo = '';
      this.sendMsg = '';
      this.showSendModal = true;
    },

    async submitSend() {
      if (!this.sendFrom || !this.sendTo || !this.sendMsg.trim()) return;
      this.sendLoading = true;
      try {
        await OpenFangAPI.post('/api/comms/send', {
          from_agent_id: this.sendFrom,
          to_agent_id: this.sendTo,
          message: this.sendMsg
        });
        OpenFangToast.success('Message sent');
        this.showSendModal = false;
      } catch(e) {
        OpenFangToast.error(e.message || 'Send failed');
      }
      this.sendLoading = false;
    },

    openTaskModal() {
      this.taskTitle = '';
      this.taskDesc = '';
      this.taskAssign = '';
      this.showTaskModal = true;
    },

    async submitTask() {
      if (!this.taskTitle.trim()) return;
      this.taskLoading = true;
      try {
        var body = { title: this.taskTitle, description: this.taskDesc };
        if (this.taskAssign) body.assigned_to = this.taskAssign;
        await OpenFangAPI.post('/api/comms/task', body);
        OpenFangToast.success('Task posted');
        this.showTaskModal = false;
      } catch(e) {
        OpenFangToast.error(e.message || 'Task failed');
      }
      this.taskLoading = false;
    },

    // ── A2A Methods ──

    async loadA2aAgents() {
      this.a2aLoading = true;
      this.a2aLoadError = '';
      try {
        var data = await OpenFangAPI.get('/api/a2a/agents');
        this.a2aAgents = data.agents || data || [];
      } catch (e) {
        this.a2aAgents = [];
        this.a2aLoadError = e.message || 'Could not load A2A agents.';
      }
      this.a2aLoading = false;
    },

    openDiscoverModal() {
      this.discoverUrl = '';
      this.showDiscoverModal = true;
    },

    async discoverAgent() {
      var url = this.discoverUrl.trim();
      if (!url) return;
      this.discoverLoading = true;
      try {
        await OpenFangAPI.post('/api/a2a/discover', { url: url });
        OpenFangToast.success('Agent discovered at ' + url);
        this.showDiscoverModal = false;
        await this.loadA2aAgents();
      } catch (e) {
        OpenFangToast.error('Discovery failed: ' + (e.message || 'Unknown error'));
      }
      this.discoverLoading = false;
    },

    openA2aSendModal() {
      this.a2aSendAgent = '';
      this.a2aSendPayload = '{\n  "message": "Hello"\n}';
      this.showA2aSendModal = true;
    },

    async sendA2aTask() {
      if (!this.a2aSendAgent) return;
      var payload;
      try {
        payload = JSON.parse(this.a2aSendPayload);
      } catch (e) {
        OpenFangToast.error('Invalid JSON payload');
        return;
      }
      this.a2aSendLoading = true;
      try {
        var result = await OpenFangAPI.post('/api/a2a/send', {
          agent_id: this.a2aSendAgent,
          payload: payload
        });
        var taskId = result.task_id || result.id;
        if (taskId) {
          OpenFangToast.success('Task sent (ID: ' + taskId.substring(0, 8) + '...)');
          this.a2aTaskStatuses[taskId] = { status: 'submitted', id: taskId };
          this.startA2aTaskPolling(taskId);
        } else {
          OpenFangToast.success('Task sent');
        }
        this.showA2aSendModal = false;
      } catch (e) {
        OpenFangToast.error('Send failed: ' + (e.message || 'Unknown error'));
      }
      this.a2aSendLoading = false;
    },

    startA2aTaskPolling(taskId) {
      var self = this;
      if (this.a2aPollingTimers[taskId]) return;
      this.a2aPollingTimers[taskId] = setInterval(async function () {
        try {
          var data = await OpenFangAPI.get('/api/a2a/tasks/' + encodeURIComponent(taskId) + '/status');
          self.a2aTaskStatuses[taskId] = data;
          var s = (data.status || '').toLowerCase();
          if (s === 'completed' || s === 'failed' || s === 'cancelled') {
            self.stopA2aTaskPolling(taskId);
          }
        } catch (e) {
          self.stopA2aTaskPolling(taskId);
        }
      }, 3000);
    },

    stopA2aTaskPolling(taskId) {
      if (this.a2aPollingTimers[taskId]) {
        clearInterval(this.a2aPollingTimers[taskId]);
        delete this.a2aPollingTimers[taskId];
      }
    },

    stopAllA2aPolling() {
      var ids = Object.keys(this.a2aPollingTimers);
      for (var i = 0; i < ids.length; i++) {
        this.stopA2aTaskPolling(ids[i]);
      }
    },

    get a2aTaskList() {
      var list = [];
      var ids = Object.keys(this.a2aTaskStatuses);
      for (var i = 0; i < ids.length; i++) {
        list.push(this.a2aTaskStatuses[ids[i]]);
      }
      return list;
    },

    a2aStatusBadgeClass(status) {
      var s = (status || '').toLowerCase();
      if (s === 'completed' || s === 'done') return 'badge badge-success';
      if (s === 'failed' || s === 'error') return 'badge badge-danger';
      if (s === 'running' || s === 'in_progress' || s === 'submitted') return 'badge badge-info';
      if (s === 'cancelled') return 'badge badge-warning';
      return 'badge badge-dim';
    }
  };
}
