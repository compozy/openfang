// OpenFang Analytics Page — Full usage analytics with per-model and per-agent breakdowns
// Includes Cost Dashboard with donut chart, bar chart, projections, and provider breakdown.
// Budget management: global and per-agent limits via /api/budget endpoints.
'use strict';

function analyticsPage() {
  return {
    tab: 'summary',
    summary: {},
    byModel: [],
    byAgent: [],
    loading: true,
    loadError: '',

    // Cost tab state
    dailyCosts: [],
    todayCost: 0,
    firstEventDate: null,

    // Budget tab state
    globalBudget: null,
    agentBudgets: [],
    editingGlobalBudget: false,
    globalBudgetForm: {
      max_hourly_usd: 0,
      max_daily_usd: 0,
      max_monthly_usd: 0,
      alert_threshold: 0.8
    },
    globalBudgetSaving: false,
    selectedAgentBudgetId: '',
    selectedAgentBudget: null,
    agentBudgetLoading: false,
    editingAgentBudget: false,
    agentBudgetForm: {
      max_cost_per_hour_usd: 0,
      max_cost_per_day_usd: 0,
      max_cost_per_month_usd: 0
    },
    agentBudgetSaving: false,

    // Chart colors for providers (stable palette)
    _chartColors: [
      '#FF5C00', '#3B82F6', '#10B981', '#F59E0B', '#8B5CF6',
      '#EC4899', '#06B6D4', '#EF4444', '#84CC16', '#F97316',
      '#6366F1', '#14B8A6', '#E11D48', '#A855F7', '#22D3EE'
    ],

    async loadUsage() {
      this.loading = true;
      this.loadError = '';
      try {
        await Promise.all([
          this.loadSummary(),
          this.loadByModel(),
          this.loadByAgent(),
          this.loadDailyCosts(),
          this.loadGlobalBudget(),
          this.loadAgentBudgets()
        ]);
      } catch(e) {
        this.loadError = e.message || 'Could not load usage data.';
      }
      this.loading = false;
    },

    async loadData() { return this.loadUsage(); },

    async loadSummary() {
      try {
        this.summary = await OpenFangAPI.get('/api/usage/summary');
      } catch(e) {
        this.summary = { total_input_tokens: 0, total_output_tokens: 0, total_cost_usd: 0, call_count: 0, total_tool_calls: 0 };
        throw e;
      }
    },

    async loadByModel() {
      try {
        var data = await OpenFangAPI.get('/api/usage/by-model');
        this.byModel = data.models || [];
      } catch(e) { this.byModel = []; }
    },

    async loadByAgent() {
      try {
        var data = await OpenFangAPI.get('/api/usage');
        this.byAgent = data.agents || [];
      } catch(e) { this.byAgent = []; }
    },

    async loadDailyCosts() {
      try {
        var data = await OpenFangAPI.get('/api/usage/daily');
        this.dailyCosts = data.days || [];
        this.todayCost = data.today_cost_usd || 0;
        this.firstEventDate = data.first_event_date || null;
      } catch(e) {
        this.dailyCosts = [];
        this.todayCost = 0;
        this.firstEventDate = null;
      }
    },

    formatTokens(n) {
      if (!n) return '0';
      if (n >= 1000000) return (n / 1000000).toFixed(2) + 'M';
      if (n >= 1000) return (n / 1000).toFixed(1) + 'K';
      return String(n);
    },

    formatCost(c) {
      if (!c) return '$0.00';
      if (c < 0.01) return '$' + c.toFixed(4);
      return '$' + c.toFixed(2);
    },

    maxTokens() {
      var max = 0;
      this.byModel.forEach(function(m) {
        var t = (m.total_input_tokens || 0) + (m.total_output_tokens || 0);
        if (t > max) max = t;
      });
      return max || 1;
    },

    barWidth(m) {
      var t = (m.total_input_tokens || 0) + (m.total_output_tokens || 0);
      return Math.max(2, Math.round((t / this.maxTokens()) * 100)) + '%';
    },

    // ── Cost tab helpers ──

    avgCostPerMessage() {
      var count = this.summary.call_count || 0;
      if (count === 0) return 0;
      return (this.summary.total_cost_usd || 0) / count;
    },

    projectedMonthlyCost() {
      if (!this.firstEventDate || !this.summary.total_cost_usd) return 0;
      var first = new Date(this.firstEventDate);
      var now = new Date();
      var diffMs = now.getTime() - first.getTime();
      var diffDays = diffMs / (1000 * 60 * 60 * 24);
      if (diffDays < 1) diffDays = 1;
      return (this.summary.total_cost_usd / diffDays) * 30;
    },

    // ── Provider aggregation from byModel data ──

    costByProvider() {
      var providerMap = {};
      var self = this;
      this.byModel.forEach(function(m) {
        var provider = self._extractProvider(m.model);
        if (!providerMap[provider]) {
          providerMap[provider] = { provider: provider, cost: 0, tokens: 0, calls: 0 };
        }
        providerMap[provider].cost += (m.total_cost_usd || 0);
        providerMap[provider].tokens += (m.total_input_tokens || 0) + (m.total_output_tokens || 0);
        providerMap[provider].calls += (m.call_count || 0);
      });
      var result = [];
      for (var key in providerMap) {
        if (providerMap.hasOwnProperty(key)) {
          result.push(providerMap[key]);
        }
      }
      result.sort(function(a, b) { return b.cost - a.cost; });
      return result;
    },

    _extractProvider(modelName) {
      if (!modelName) return 'Unknown';
      var lower = modelName.toLowerCase();
      if (lower.indexOf('claude') !== -1 || lower.indexOf('haiku') !== -1 || lower.indexOf('sonnet') !== -1 || lower.indexOf('opus') !== -1) return 'Anthropic';
      if (lower.indexOf('gemini') !== -1 || lower.indexOf('gemma') !== -1) return 'Google';
      if (lower.indexOf('gpt') !== -1 || lower.indexOf('o1') !== -1 || lower.indexOf('o3') !== -1 || lower.indexOf('o4') !== -1) return 'OpenAI';
      if (lower.indexOf('llama') !== -1 || lower.indexOf('mixtral') !== -1 || lower.indexOf('groq') !== -1) return 'Groq';
      if (lower.indexOf('deepseek') !== -1) return 'DeepSeek';
      if (lower.indexOf('mistral') !== -1) return 'Mistral';
      if (lower.indexOf('command') !== -1 || lower.indexOf('cohere') !== -1) return 'Cohere';
      if (lower.indexOf('grok') !== -1) return 'xAI';
      if (lower.indexOf('jamba') !== -1) return 'AI21';
      if (lower.indexOf('qwen') !== -1) return 'Together';
      return 'Other';
    },

    // ── Donut chart (stroke-dasharray on circles) ──

    donutSegments() {
      var providers = this.costByProvider();
      var total = 0;
      var colors = this._chartColors;
      providers.forEach(function(p) { total += p.cost; });
      if (total === 0) return [];

      var segments = [];
      var offset = 0;
      var circumference = 2 * Math.PI * 60; // r=60
      for (var i = 0; i < providers.length; i++) {
        var pct = providers[i].cost / total;
        var dashLen = pct * circumference;
        segments.push({
          provider: providers[i].provider,
          cost: providers[i].cost,
          percent: Math.round(pct * 100),
          color: colors[i % colors.length],
          dasharray: dashLen + ' ' + (circumference - dashLen),
          dashoffset: -offset,
          circumference: circumference
        });
        offset += dashLen;
      }
      return segments;
    },

    // ── Bar chart (last 7 days) ──

    barChartData() {
      var days = this.dailyCosts;
      if (!days || days.length === 0) return [];
      var maxCost = 0;
      days.forEach(function(d) { if (d.cost_usd > maxCost) maxCost = d.cost_usd; });
      if (maxCost === 0) maxCost = 1;

      var dayNames = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
      var result = [];
      for (var i = 0; i < days.length; i++) {
        var d = new Date(days[i].date + 'T12:00:00');
        var dayName = dayNames[d.getDay()] || '?';
        var heightPct = Math.max(2, Math.round((days[i].cost_usd / maxCost) * 120));
        result.push({
          date: days[i].date,
          dayName: dayName,
          cost: days[i].cost_usd,
          tokens: days[i].tokens,
          calls: days[i].calls,
          barHeight: heightPct
        });
      }
      return result;
    },

    // ── Cost by model table (sorted by cost descending) ──

    costByModelSorted() {
      var models = this.byModel.slice();
      models.sort(function(a, b) { return (b.total_cost_usd || 0) - (a.total_cost_usd || 0); });
      return models;
    },

    maxModelCost() {
      var max = 0;
      this.byModel.forEach(function(m) {
        if ((m.total_cost_usd || 0) > max) max = m.total_cost_usd;
      });
      return max || 1;
    },

    costBarWidth(m) {
      return Math.max(2, Math.round(((m.total_cost_usd || 0) / this.maxModelCost()) * 100)) + '%';
    },

    modelTier(modelName) {
      if (!modelName) return 'unknown';
      var lower = modelName.toLowerCase();
      if (lower.indexOf('opus') !== -1 || lower.indexOf('o1') !== -1 || lower.indexOf('o3') !== -1 || lower.indexOf('deepseek-r1') !== -1) return 'frontier';
      if (lower.indexOf('sonnet') !== -1 || lower.indexOf('gpt-4') !== -1 || lower.indexOf('gemini-2.5') !== -1 || lower.indexOf('gemini-1.5-pro') !== -1) return 'smart';
      if (lower.indexOf('haiku') !== -1 || lower.indexOf('gpt-3.5') !== -1 || lower.indexOf('flash') !== -1 || lower.indexOf('mixtral') !== -1) return 'balanced';
      if (lower.indexOf('llama') !== -1 || lower.indexOf('groq') !== -1 || lower.indexOf('gemma') !== -1) return 'fast';
      return 'balanced';
    },

    // ── Budget tab methods ──

    async loadGlobalBudget() {
      try {
        this.globalBudget = await OpenFangAPI.get('/api/budget');
      } catch(e) {
        this.globalBudget = null;
      }
    },

    async loadAgentBudgets() {
      try {
        var data = await OpenFangAPI.get('/api/budget/agents');
        this.agentBudgets = data.agents || [];
      } catch(e) {
        this.agentBudgets = [];
      }
    },

    formatBudgetLimit(v) {
      if (!v || v === 0) return 'Unlimited';
      return '$' + v.toFixed(2);
    },

    budgetPctClass(pct) {
      if (!pct || pct <= 0) return '';
      if (pct >= 1.0) return 'budget-bar-danger';
      if (pct >= 0.8) return 'budget-bar-warn';
      return 'budget-bar-ok';
    },

    budgetPctWidth(pct) {
      if (!pct || pct <= 0) return '0%';
      return Math.min(100, Math.round(pct * 100)) + '%';
    },

    openEditGlobalBudget() {
      var b = this.globalBudget || {};
      this.globalBudgetForm = {
        max_hourly_usd: b.hourly_limit || 0,
        max_daily_usd: b.daily_limit || 0,
        max_monthly_usd: b.monthly_limit || 0,
        alert_threshold: b.alert_threshold || 0.8
      };
      this.editingGlobalBudget = true;
    },

    async saveGlobalBudget() {
      this.globalBudgetSaving = true;
      try {
        this.globalBudget = await OpenFangAPI.put('/api/budget', {
          max_hourly_usd: parseFloat(this.globalBudgetForm.max_hourly_usd) || 0,
          max_daily_usd: parseFloat(this.globalBudgetForm.max_daily_usd) || 0,
          max_monthly_usd: parseFloat(this.globalBudgetForm.max_monthly_usd) || 0,
          alert_threshold: parseFloat(this.globalBudgetForm.alert_threshold) || 0.8
        });
        this.editingGlobalBudget = false;
      } catch(e) {
        alert('Failed to save budget: ' + (e.message || e));
      } finally {
        this.globalBudgetSaving = false;
      }
    },

    async openAgentBudgetDetail(agentId) {
      this.selectedAgentBudgetId = agentId;
      this.selectedAgentBudget = null;
      this.editingAgentBudget = false;
      this.agentBudgetLoading = true;
      try {
        this.selectedAgentBudget = await OpenFangAPI.get(
          '/api/budget/agents/' + encodeURIComponent(agentId)
        );
      } catch(e) {
        this.selectedAgentBudget = null;
      } finally {
        this.agentBudgetLoading = false;
      }
    },

    closeAgentBudgetDetail() {
      this.selectedAgentBudgetId = '';
      this.selectedAgentBudget = null;
      this.editingAgentBudget = false;
    },

    openEditAgentBudget() {
      var a = this.selectedAgentBudget || {};
      this.agentBudgetForm = {
        max_cost_per_hour_usd: (a.hourly && a.hourly.limit) || 0,
        max_cost_per_day_usd: (a.daily && a.daily.limit) || 0,
        max_cost_per_month_usd: (a.monthly && a.monthly.limit) || 0
      };
      this.editingAgentBudget = true;
    },

    async saveAgentBudget() {
      this.agentBudgetSaving = true;
      try {
        await OpenFangAPI.put(
          '/api/budget/agents/' + encodeURIComponent(this.selectedAgentBudgetId),
          {
            max_cost_per_hour_usd:
              parseFloat(this.agentBudgetForm.max_cost_per_hour_usd) || 0,
            max_cost_per_day_usd:
              parseFloat(this.agentBudgetForm.max_cost_per_day_usd) || 0,
            max_cost_per_month_usd:
              parseFloat(this.agentBudgetForm.max_cost_per_month_usd) || 0
          }
        );
        this.editingAgentBudget = false;
        await Promise.all([
          this.openAgentBudgetDetail(this.selectedAgentBudgetId),
          this.loadAgentBudgets()
        ]);
      } catch(e) {
        alert('Failed to save agent budget: ' + (e.message || e));
      } finally {
        this.agentBudgetSaving = false;
      }
    }
  };
}
