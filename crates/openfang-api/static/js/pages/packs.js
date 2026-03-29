// OpenFang Packs Page — pack lifecycle management with install, upgrade, uninstall, fork
'use strict';

function packsPage() {
  return {
    packs: [],
    loading: true,
    loadError: '',
    selectedPackId: '',
    selectedPack: null,
    objects: [],
    detailLoading: false,
    detailError: '',
    selectedTab: 'detail',
    clockTimer: null,
    nowTick: Date.now(),

    // Install modal
    showInstallModal: false,
    installForm: { kind: 'bundled', pack_id: '', version: '' },
    installSubmitting: false,

    // Upgrade modal
    showUpgradeModal: false,
    upgradePackId: '',
    upgradeForm: { target_version: '' },
    dryRunResult: null,
    dryRunLoading: false,
    upgradeSubmitting: false,

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
      this.loading = true;
      this.loadError = '';
      try {
        var response = await OpenFangAPI.v1.packs.list();
        this.packs = this.normalizeList(response);
        if (this.selectedPackId) {
          var found = false;
          for (var i = 0; i < this.packs.length; i++) {
            if (this.packs[i].id === this.selectedPackId) { found = true; break; }
          }
          if (found) {
            await this.refreshDetail();
          } else {
            this.clearSelection();
          }
        }
      } catch (e) {
        this.packs = [];
        this.loadError = e.message || 'Could not load packs.';
      }
      this.loading = false;
    },

    normalizeList(payload) {
      if (Array.isArray(payload)) return payload;
      if (payload && Array.isArray(payload.items)) return payload.items;
      return [];
    },

    async selectPack(id) {
      if (!id) { this.clearSelection(); return; }
      if (id === this.selectedPackId && this.selectedPack) return;
      this.selectedPackId = id;
      this.selectedTab = 'detail';
      this.selectedPack = null;
      this.objects = [];
      this.detailError = '';
      await this.refreshDetail();
    },

    async refreshDetail() {
      var id = this.selectedPackId;
      if (!id) return;
      this.detailLoading = true;
      this.detailError = '';
      try {
        var results = await Promise.all([
          OpenFangAPI.v1.packs.get(id),
          OpenFangAPI.v1.packs.objects(id)
        ]);
        if (this.selectedPackId !== id) return;
        this.selectedPack = results[0];
        this.objects = this.normalizeList(results[1]);
      } catch (e) {
        if (this.selectedPackId !== id) return;
        this.detailError = e.message || 'Could not load pack details.';
      }
      this.detailLoading = false;
    },

    clearSelection() {
      this.selectedPackId = '';
      this.selectedPack = null;
      this.objects = [];
      this.detailError = '';
      this.detailLoading = false;
      this.selectedTab = 'detail';
    },

    // ── Badge & display helpers ──

    sourceBadgeClass(source) {
      if (!source || !source.kind) return 'badge badge-dim';
      if (source.kind === 'bundled') return 'badge badge-success';
      return 'badge badge-info';
    },

    sourceBadgeText(source) {
      if (!source || !source.kind) return 'Unknown';
      if (source.kind === 'bundled') return 'Bundled';
      return 'External';
    },

    objectCountSummary(counts) {
      if (!counts) return '0 objects';
      var total = (counts.agents || 0) + (counts.workflows || 0)
        + (counts.triggers || 0) + (counts.schedules || 0) + (counts.templates || 0);
      if (total === 0) return '0 objects';
      var parts = [];
      if (counts.agents) parts.push(counts.agents + ' agent' + (counts.agents > 1 ? 's' : ''));
      if (counts.workflows) parts.push(counts.workflows + ' workflow' + (counts.workflows > 1 ? 's' : ''));
      if (counts.triggers) parts.push(counts.triggers + ' trigger' + (counts.triggers > 1 ? 's' : ''));
      if (counts.schedules) parts.push(counts.schedules + ' schedule' + (counts.schedules > 1 ? 's' : ''));
      if (counts.templates) parts.push(counts.templates + ' template' + (counts.templates > 1 ? 's' : ''));
      return parts.join(', ');
    },

    objectTotal(counts) {
      if (!counts) return 0;
      return (counts.agents || 0) + (counts.workflows || 0)
        + (counts.triggers || 0) + (counts.schedules || 0) + (counts.templates || 0);
    },

    resourceTypeLabel(type_) {
      if (!type_) return 'Unknown';
      return String(type_).replace(/_/g, ' ')
        .replace(/\b\w/g, function(l) { return l.toUpperCase(); });
    },

    resourceTypeBadgeClass(type_) {
      if (!type_) return 'badge badge-dim';
      switch (type_) {
        case 'agent': return 'badge badge-run-running';
        case 'workflow': return 'badge badge-run-pending';
        case 'trigger': return 'badge badge-run-waiting-signal';
        case 'schedule': return 'badge badge-run-paused';
        case 'template': return 'badge badge-created';
        default: return 'badge badge-dim';
      }
    },

    forkedBadgeClass(forked) {
      return forked ? 'badge badge-warn' : 'badge badge-dim';
    },

    statusBadgeClass(status) {
      return OpenFangUtils.statusBadge(status);
    },

    formatDateTime(value) {
      return OpenFangUtils.formatDateTime(value);
    },

    relativeTime(value) {
      var tick = this.nowTick;
      if (tick < 0) return '';
      return OpenFangUtils.timeAgo(value);
    },

    // ── Install ──

    openInstallModal() {
      this.showInstallModal = true;
      this.installForm = { kind: 'bundled', pack_id: '', version: '' };
      this.installSubmitting = false;
    },

    closeInstallModal() {
      this.showInstallModal = false;
    },

    async submitInstall() {
      if (!this.installForm.pack_id.trim()) {
        OpenFangToast.error('Pack ID is required.');
        return;
      }
      if (!this.installForm.version.trim()) {
        OpenFangToast.error('Version is required.');
        return;
      }
      this.installSubmitting = true;
      try {
        await OpenFangAPI.v1.packs.install({
          source: {
            kind: this.installForm.kind,
            pack_id: this.installForm.pack_id.trim(),
            version: this.installForm.version.trim()
          }
        });
        OpenFangToast.success('Pack installed successfully.');
        this.showInstallModal = false;
        await this.loadData();
      } catch (e) {
        OpenFangToast.error(e.message || 'Failed to install pack.');
      }
      this.installSubmitting = false;
    },

    // ── Upgrade (dry-run + commit) ──

    openUpgradeModal(packId) {
      this.upgradePackId = packId;
      this.upgradeForm = { target_version: '' };
      this.dryRunResult = null;
      this.dryRunLoading = false;
      this.upgradeSubmitting = false;
      this.showUpgradeModal = true;
    },

    closeUpgradeModal() {
      this.showUpgradeModal = false;
      this.dryRunResult = null;
    },

    async runDryRun() {
      if (!this.upgradeForm.target_version.trim()) {
        OpenFangToast.error('Target version is required.');
        return;
      }
      this.dryRunLoading = true;
      this.dryRunResult = null;
      try {
        var result = await OpenFangAPI.v1.packs.upgradeDryRun(this.upgradePackId, {
          target_version: this.upgradeForm.target_version.trim()
        });
        this.dryRunResult = result;
      } catch (e) {
        OpenFangToast.error(e.message || 'Dry-run failed.');
      }
      this.dryRunLoading = false;
    },

    async confirmUpgrade() {
      this.upgradeSubmitting = true;
      try {
        await OpenFangAPI.v1.packs.upgrade(this.upgradePackId, {
          target_version: this.upgradeForm.target_version.trim()
        });
        OpenFangToast.success('Pack upgraded successfully.');
        this.showUpgradeModal = false;
        this.dryRunResult = null;
        await this.loadData();
      } catch (e) {
        OpenFangToast.error(e.message || 'Upgrade failed.');
      }
      this.upgradeSubmitting = false;
    },

    // ── Uninstall ──

    uninstallPack(packId) {
      var self = this;
      var pack = null;
      for (var i = 0; i < this.packs.length; i++) {
        if (this.packs[i].id === packId) { pack = this.packs[i]; break; }
      }

      // Check if any objects are forked
      var hasForkedObjects = false;
      for (var j = 0; j < this.objects.length; j++) {
        if (this.objects[j].forked) { hasForkedObjects = true; break; }
      }

      var message = 'Uninstall pack "' + (pack ? pack.name : packId) + '"?';
      if (hasForkedObjects) {
        message += '\n\nWarning: This pack has forked objects. '
          + 'Forked objects will remain as user-owned copies after uninstall.';
      }
      message += '\n\nThis will remove all managed objects that have not been forked.';

      OpenFangUtils.confirmAction('Uninstall Pack', message, async function() {
        try {
          await OpenFangAPI.v1.packs.uninstall(packId, { force: false });
          OpenFangToast.success('Pack uninstalled successfully.');
          self.clearSelection();
          await self.loadData();
        } catch (e) {
          OpenFangToast.error(e.message || 'Uninstall failed.');
        }
      });
    },

    // ── Fork ──

    forkObject(packId, resourceType, resourceId) {
      var self = this;
      OpenFangUtils.confirmAction(
        'Fork Object',
        'Create a user-owned copy of this ' + resourceType + ' ("' + resourceId + '")? '
          + 'The forked copy will be independent from pack upgrades.',
        async function() {
          try {
            await OpenFangAPI.v1.packs.fork(packId, {
              resource_type: resourceType,
              resource_id: resourceId,
              mode: 'shadow'
            });
            OpenFangToast.success('Object forked successfully.');
            await self.refreshDetail();
          } catch (e) {
            OpenFangToast.error(e.message || 'Fork failed.');
          }
        }
      );
    },

    // ── Tab switching ──

    setTab(tab) {
      this.selectedTab = tab;
    },

    // ── Computed ──

    get forkedCount() {
      var count = 0;
      for (var i = 0; i < this.objects.length; i++) {
        if (this.objects[i].forked) count++;
      }
      return count;
    }
  };
}
