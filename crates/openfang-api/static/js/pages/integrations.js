// OpenFang Integrations Page — list, add, remove, reconnect, health
'use strict';

function integrationsPage() {
  return {
    integrations: [],
    available: [],
    healthMap: {},
    loading: true,
    loadError: '',
    searchFilter: '',
    showAddModal: false,
    addType: '',
    addConfig: {},
    addLoading: false,
    reconnecting: {},
    removing: {},
    refreshTimer: null,

    async loadData() {
      this.loading = true;
      this.loadError = '';
      try {
        await Promise.all([
          this.loadIntegrations(),
          this.loadAvailable(),
          this.loadHealth()
        ]);
      } catch (e) {
        this.loadError = e.message || 'Could not load integrations.';
      }
      this.loading = false;
    },

    async loadIntegrations() {
      try {
        var data = await OpenFangAPI.get('/api/integrations');
        this.integrations = data.integrations || data || [];
      } catch (e) { this.integrations = []; throw e; }
    },

    async loadAvailable() {
      try {
        var data = await OpenFangAPI.get('/api/integrations/available');
        this.available = data.types || data.available || data || [];
      } catch (e) { this.available = []; }
    },

    async loadHealth() {
      try {
        var data = await OpenFangAPI.get('/api/integrations/health');
        var map = {};
        var items = data.health || data || [];
        if (Array.isArray(items)) {
          for (var i = 0; i < items.length; i++) {
            var h = items[i];
            if (h.id) map[h.id] = h;
          }
        } else if (typeof items === 'object') {
          map = items;
        }
        this.healthMap = map;
      } catch (e) { this.healthMap = {}; }
    },

    startAutoRefresh() {
      this.stopAutoRefresh();
      var self = this;
      this.refreshTimer = setInterval(function () {
        self.loadHealth().catch(function () {});
      }, 30000);
    },

    stopAutoRefresh() {
      if (this.refreshTimer) {
        clearInterval(this.refreshTimer);
        this.refreshTimer = null;
      }
    },

    destroy() {
      this.stopAutoRefresh();
    },

    get filteredIntegrations() {
      var q = this.searchFilter.toLowerCase().trim();
      if (!q) return this.integrations;
      return this.integrations.filter(function (it) {
        return (it.name || '').toLowerCase().indexOf(q) !== -1 ||
               (it.type || '').toLowerCase().indexOf(q) !== -1 ||
               (it.id || '').toLowerCase().indexOf(q) !== -1;
      });
    },

    get availableToAdd() {
      var existing = {};
      for (var i = 0; i < this.integrations.length; i++) {
        existing[this.integrations[i].type] = true;
      }
      return this.available.filter(function (a) {
        return !existing[a.type || a.id || a.name];
      });
    },

    healthOf(id) {
      return this.healthMap[id] || null;
    },

    healthBadgeClass(id) {
      var h = this.healthOf(id);
      if (!h) return 'badge badge-dim';
      var s = (h.status || h.state || '').toLowerCase();
      if (s === 'healthy' || s === 'connected' || s === 'ok') return 'badge badge-success';
      if (s === 'degraded' || s === 'warning') return 'badge badge-warning';
      if (s === 'unhealthy' || s === 'error' || s === 'disconnected') return 'badge badge-danger';
      return 'badge badge-dim';
    },

    healthLabel(id) {
      var h = this.healthOf(id);
      if (!h) return 'Unknown';
      return h.status || h.state || 'Unknown';
    },

    openAddModal() {
      this.addType = '';
      this.addConfig = {};
      this.showAddModal = true;
    },

    selectAddType(type) {
      this.addType = type.type || type.id || type.name || '';
      this.addConfig = {};
    },

    async addIntegration() {
      if (!this.addType) return;
      this.addLoading = true;
      try {
        await OpenFangAPI.post('/api/integrations/add', {
          type: this.addType,
          config: this.addConfig
        });
        OpenFangToast.success('Integration added: ' + this.addType);
        this.showAddModal = false;
        await this.loadData();
      } catch (e) {
        OpenFangToast.error('Failed to add integration: ' + (e.message || 'Unknown error'));
      }
      this.addLoading = false;
    },

    removeIntegration(integration) {
      var self = this;
      var id = integration.id;
      var name = integration.name || integration.type || id;
      OpenFangToast.confirm('Remove Integration', 'Remove "' + name + '"? This cannot be undone.', async function () {
        self.removing[id] = true;
        try {
          await OpenFangAPI.del('/api/integrations/' + encodeURIComponent(id));
          OpenFangToast.success('Integration removed: ' + name);
          self.integrations = self.integrations.filter(function (it) { return it.id !== id; });
        } catch (e) {
          OpenFangToast.error('Failed to remove: ' + (e.message || 'Unknown error'));
        }
        self.removing[id] = false;
      });
    },

    async reconnectIntegration(integration) {
      var id = integration.id;
      this.reconnecting[id] = true;
      try {
        await OpenFangAPI.post('/api/integrations/' + encodeURIComponent(id) + '/reconnect', {});
        OpenFangToast.success('Reconnecting: ' + (integration.name || integration.type || id));
        await this.loadHealth();
      } catch (e) {
        OpenFangToast.error('Reconnect failed: ' + (e.message || 'Unknown error'));
      }
      this.reconnecting[id] = false;
    },

    integrationIcon(type) {
      var t = (type || '').toLowerCase();
      if (t === 'database' || t === 'db' || t === 'postgres' || t === 'sqlite') return 'db';
      if (t === 'webhook' || t === 'http') return 'webhook';
      if (t === 'email' || t === 'smtp') return 'email';
      if (t === 'slack' || t === 'discord' || t === 'telegram') return 'chat';
      if (t === 'github' || t === 'gitlab' || t === 'git') return 'code';
      if (t === 'storage' || t === 's3' || t === 'gcs') return 'storage';
      return 'default';
    },

    typeLabel(type) {
      if (!type) return '-';
      return type.charAt(0).toUpperCase() + type.slice(1).replace(/[_-]/g, ' ');
    },

    timeAgo(dateStr) {
      if (!dateStr) return '';
      var d = new Date(dateStr);
      var secs = Math.floor((Date.now() - d.getTime()) / 1000);
      if (secs < 60) return secs + 's ago';
      if (secs < 3600) return Math.floor(secs / 60) + 'm ago';
      if (secs < 86400) return Math.floor(secs / 3600) + 'h ago';
      return Math.floor(secs / 86400) + 'd ago';
    }
  };
}
