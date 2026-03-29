// OpenFang Artifacts Page — browse artifacts with type/task filters, detail, version history
'use strict';

function artifactsPage() {
  return {
    artifacts: [],
    loading: true,
    loadError: '',
    selectedId: '',
    selectedArtifact: null,
    versions: [],
    detailLoading: false,
    detailError: '',
    selectedTab: 'detail',
    filterType: '',
    filterTaskId: '',
    searchQuery: '',
    listRequestToken: 0,
    clockTimer: null,
    nowTick: Date.now(),

    // ── Lifecycle ──

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
    },

    // ── Data loading ──

    async loadData() {
      return this.loadArtifacts({ refreshDetail: true });
    },

    normalizeList(payload) {
      if (Array.isArray(payload)) return payload;
      if (payload && Array.isArray(payload.items)) return payload.items;
      return [];
    },

    queryParams() {
      var params = { limit: 200 };
      if (this.filterType) params.artifact_type = this.filterType;
      if (this.filterTaskId.trim()) params.task_id = this.filterTaskId.trim();
      if (this.searchQuery.trim()) params.q = this.searchQuery.trim();
      return params;
    },

    async loadArtifacts(options) {
      options = options || {};
      var requestToken = ++this.listRequestToken;

      if (!options.silent) {
        this.loading = true;
      }
      this.loadError = '';

      try {
        var response = await OpenFangAPI.v1.artifacts.list(this.queryParams());
        if (requestToken !== this.listRequestToken) return;

        this.artifacts = this.sortByUpdated(this.normalizeList(response));

        if (this.selectedId) {
          var found = false;
          for (var i = 0; i < this.artifacts.length; i++) {
            if (this.artifacts[i].id === this.selectedId) { found = true; break; }
          }
          if (found && options.refreshDetail !== false) {
            await this.refreshDetail();
          } else if (!found) {
            this.clearSelection();
          }
        }
      } catch (e) {
        if (requestToken !== this.listRequestToken) return;
        this.artifacts = [];
        this.loadError = e.message || 'Could not load artifacts.';
        this.clearSelection();
      }

      if (!options.silent) {
        this.loading = false;
      }
    },

    sortByUpdated(items) {
      return items.slice().sort(function(a, b) {
        var ta = a.updated_at || a.created_at || '';
        var tb = b.updated_at || b.created_at || '';
        return tb.localeCompare(ta);
      });
    },

    // ── Selection ──

    async selectArtifact(id) {
      if (!id) { this.clearSelection(); return; }
      if (id === this.selectedId && this.selectedArtifact) return;
      this.selectedId = id;
      this.selectedTab = 'detail';
      this.selectedArtifact = null;
      this.versions = [];
      this.detailError = '';
      await this.refreshDetail();
    },

    async refreshDetail() {
      var id = this.selectedId;
      if (!id) return;
      this.detailLoading = true;
      this.detailError = '';
      try {
        var results = await Promise.all([
          OpenFangAPI.v1.artifacts.get(id),
          OpenFangAPI.v1.artifacts.versions(id)
        ]);
        if (this.selectedId !== id) return;
        this.selectedArtifact = results[0];
        this.versions = this.normalizeList(results[1]);
      } catch (e) {
        if (this.selectedId !== id) return;
        this.detailError = e.message || 'Could not load artifact details.';
      }
      this.detailLoading = false;
    },

    clearSelection() {
      this.selectedId = '';
      this.selectedArtifact = null;
      this.versions = [];
      this.detailError = '';
      this.detailLoading = false;
      this.selectedTab = 'detail';
    },

    // ── Filters ──

    applyFilters() {
      this.clearSelection();
      this.loadArtifacts({});
    },

    clearFilters() {
      this.filterType = '';
      this.filterTaskId = '';
      this.searchQuery = '';
      this.applyFilters();
    },

    get hasActiveFilters() {
      return !!(this.filterType || this.filterTaskId.trim() || this.searchQuery.trim());
    },

    // ── Provenance navigation ──

    provenanceLabel(prov) {
      if (!prov) return '-';
      var kind = String(prov.kind || '');
      var ref = String(prov.ref || '');
      if (!kind && !ref) return '-';
      return kind + ': ' + ref;
    },

    provenancePage(prov) {
      if (!prov || !prov.kind) return '';
      switch (prov.kind) {
        case 'dispatch': return 'dispatches';
        case 'run': return 'runs';
        case 'looper_run': return 'looper';
        case 'agent': return 'agents';
        default: return '';
      }
    },

    navigateToProvenance(prov) {
      var page = this.provenancePage(prov);
      if (page) {
        window.location.hash = page;
      }
    },

    hasProvenanceLink(prov) {
      return !!this.provenancePage(prov);
    },

    // ── Display helpers ──

    typeBadgeClass(type_) {
      if (!type_) return 'badge badge-dim';
      return 'badge badge-info';
    },

    formatType(type_) {
      if (!type_) return 'Unknown';
      return String(type_).replace(/_/g, ' ')
        .replace(/\b\w/g, function(l) { return l.toUpperCase(); });
    },

    truncateHash(hash) {
      if (!hash) return '-';
      var s = String(hash);
      if (s.length <= 16) return s;
      return s.substring(0, 8) + '\u2026' + s.substring(s.length - 8);
    },

    formatContentPreview(artifact) {
      if (!artifact || !artifact.current_version) return 'No content available';
      var cv = artifact.current_version;
      var parts = [];
      parts.push('Version ' + (cv.version_number || '?'));
      if (cv.content_hash) {
        parts.push('Hash: ' + this.truncateHash(cv.content_hash));
      }
      return parts.join(' \u2022 ');
    },

    setTab(tab) {
      this.selectedTab = tab;
    },

    formatDateTime(value) {
      return OpenFangUtils.formatDateTime(value);
    },

    relativeTime(value) {
      var tick = this.nowTick;
      if (tick < 0) return '';
      return OpenFangUtils.timeAgo(value);
    }
  };
}
