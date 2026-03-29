// OpenFang Agents Page — v1 API migration
// Structured forms, validate/compile, runtime panel, sessions, skills/MCP
'use strict';

// ── Helpers ──

// ── Arky Driver Definitions ──

var ARKY_DRIVERS = [
  { id: 'codex', label: 'Codex (OpenAI CLI)', group: 'direct' },
  { id: 'claude-code', label: 'Claude Code (CLI)', group: 'direct' },
  { id: 'openrouter', label: 'OpenRouter', group: 'gateway' },
  { id: 'bedrock', label: 'AWS Bedrock', group: 'gateway' },
  { id: 'vertex', label: 'GCP Vertex AI', group: 'gateway' },
  { id: 'ollama', label: 'Ollama (Local)', group: 'gateway' },
  { id: 'zai', label: 'Zai', group: 'gateway' },
  { id: 'vercel', label: 'Vercel AI', group: 'gateway' },
  { id: 'moonshot', label: 'Moonshot', group: 'gateway' },
  { id: 'minimax', label: 'MiniMax', group: 'gateway' }
];

var REASONING_EFFORT_OPTIONS = [
  { value: '', label: 'None (default)' },
  { value: 'low', label: 'Low' },
  { value: 'medium', label: 'Medium' },
  { value: 'high', label: 'High' },
  { value: 'xhigh', label: 'XHigh' }
];

function arkyDriverConfigFields(driver) {
  if (driver === 'codex') {
    return [
      { key: 'sandbox_mode', label: 'Sandbox Mode', type: 'select', options: ['safe', 'unsafe'] },
      { key: 'web_search', label: 'Web Search', type: 'toggle' },
      { key: 'reasoning_summary', label: 'Reasoning Summary', type: 'select', options: ['off', 'brief', 'full'] }
    ];
  }
  if (driver === 'claude-code') {
    return [
      { key: 'allowed_tools', label: 'Allowed Tools', type: 'text', placeholder: 'comma-separated tool names' },
      { key: 'disallowed_tools', label: 'Disallowed Tools', type: 'text', placeholder: 'comma-separated tool names' },
      { key: 'max_budget_usd', label: 'Max Budget (USD)', type: 'number' },
      { key: 'fallback_model', label: 'Fallback Model', type: 'text', placeholder: 'model-id' },
      { key: 'mcp_servers', label: 'Inline MCP Servers', type: 'text', placeholder: 'comma-separated server names' }
    ];
  }
  if (driver === 'bedrock' || driver === 'vertex') {
    var fields = [
      { key: 'selected_model', label: 'Model ID', type: 'text', placeholder: 'provider-specific model ID' },
      { key: 'region', label: 'Region', type: 'text', placeholder: 'e.g. us-east-1' }
    ];
    if (driver === 'vertex') {
      fields.push({ key: 'project_id', label: 'Project ID', type: 'text', placeholder: 'GCP project ID' });
    }
    return fields;
  }
  // Gateway claude-compatible drivers
  if (['openrouter', 'ollama', 'zai', 'vercel', 'moonshot', 'minimax'].indexOf(driver) !== -1) {
    return [
      { key: 'selected_model', label: 'Model ID', type: 'text', placeholder: 'provider-specific model ID' },
      { key: 'allowed_tools', label: 'Allowed Tools', type: 'text', placeholder: 'comma-separated tool names' },
      { key: 'disallowed_tools', label: 'Disallowed Tools', type: 'text', placeholder: 'comma-separated tool names' },
      { key: 'max_budget_usd', label: 'Max Budget (USD)', type: 'number' },
      { key: 'fallback_model', label: 'Fallback Model', type: 'text', placeholder: 'model-id' }
    ];
  }
  return [];
}

function agentUiDefaultForm() {
  return {
    name: '',
    description: '',
    module: 'builtin:chat',
    enabled: true,
    group: '',
    tags_text: '',
    profile: 'full',
    provider: 'groq',
    model: 'llama-3.3-70b-versatile',
    system_prompt: 'You are a helpful assistant.',
    // Arky provider fields
    arky_driver: '',
    arky_profile_id: '',
    reasoning_effort: '',
    max_tokens: '',
    driver_config: {},
    emoji: '',
    color: '#FF5C00',
    archetype: '',
    vibe: '',
    caps_memory_read: true,
    caps_memory_write: true,
    caps_network: false,
    caps_shell: false,
    caps_agent_spawn: false
  };
}

function agentUiBuildDefinition(form) {
  var tags = form.tags_text
    ? form.tags_text.split(',').map(function(t) { return t.trim(); }).filter(Boolean)
    : [];
  var def = {
    name: (form.name || '').trim(),
    description: (form.description || '').trim(),
    module: form.module || 'builtin:chat',
    enabled: form.enabled !== false,
    profile: form.profile || 'full',
    model: {
      provider: (form.provider || '').trim(),
      model: (form.model || '').trim(),
      system_prompt: form.system_prompt || ''
    }
  };
  if (form.group && form.group.trim()) {
    def.group = form.group.trim();
  }
  if (tags.length) {
    def.tags = tags;
  }
  if (form.emoji || form.color || form.archetype || form.vibe) {
    def.identity = {};
    if (form.emoji) def.identity.emoji = form.emoji;
    if (form.color) def.identity.color = form.color;
    if (form.archetype) def.identity.archetype = form.archetype;
    if (form.vibe) def.identity.vibe = form.vibe;
  }
  if (form.profile === 'custom') {
    def.capabilities = {};
    if (form.caps_memory_read) def.capabilities.memory_read = ['*'];
    if (form.caps_memory_write) def.capabilities.memory_write = ['self.*'];
    if (form.caps_network) def.capabilities.network = ['*'];
    if (form.caps_shell) def.capabilities.shell = ['*'];
    if (form.caps_agent_spawn) def.capabilities.agent_spawn = true;
  }
  // Arky provider block
  if (form.arky_driver) {
    def.provider = { driver: form.arky_driver };
    if (form.model.trim()) def.provider.model = form.model.trim();
    if (form.arky_profile_id) def.provider.profile = form.arky_profile_id;
    var defaults = {};
    if (form.reasoning_effort) defaults.reasoning_effort = form.reasoning_effort;
    if (form.max_tokens && parseInt(form.max_tokens, 10) > 0) {
      defaults.max_tokens = parseInt(form.max_tokens, 10);
    }
    if (Object.keys(defaults).length) def.provider.defaults = defaults;
    if (form.driver_config && Object.keys(form.driver_config).length) {
      var cleanConfig = {};
      var configKeys = Object.keys(form.driver_config);
      for (var ci = 0; ci < configKeys.length; ci++) {
        var ck = configKeys[ci];
        var cv = form.driver_config[ck];
        if (cv !== '' && cv !== null && cv !== undefined) {
          cleanConfig[ck] = cv;
        }
      }
      if (Object.keys(cleanConfig).length) def.provider.config = cleanConfig;
    }
  }
  return def;
}

function agentUiHydrateForm(resource) {
  var def = resource;
  if (resource && resource.definition) def = resource.definition;
  var form = agentUiDefaultForm();
  if (!def) return form;

  form.name = def.name || '';
  form.description = def.description || '';
  form.module = def.module || 'builtin:chat';
  form.enabled = def.enabled !== false;
  form.group = def.group || '';
  form.tags_text = Array.isArray(def.tags) ? def.tags.join(', ') : '';
  form.profile = def.profile || 'full';

  if (def.model) {
    form.provider = def.model.provider || def.model_provider || 'groq';
    form.model = def.model.model || def.model_name || '';
    form.system_prompt = def.model.system_prompt || def.system_prompt || '';
  } else {
    form.provider = def.model_provider || 'groq';
    form.model = def.model_name || '';
    form.system_prompt = def.system_prompt || '';
  }

  // Arky provider fields
  if (def.provider && typeof def.provider === 'object') {
    form.arky_driver = def.provider.driver || '';
    form.arky_profile_id = def.provider.profile || '';
    if (def.provider.model) form.model = def.provider.model;
    if (def.provider.defaults) {
      form.reasoning_effort = def.provider.defaults.reasoning_effort || '';
      form.max_tokens = def.provider.defaults.max_tokens ? String(def.provider.defaults.max_tokens) : '';
    }
    form.driver_config = Object.assign({}, def.provider.config || {});
  }

  if (def.identity) {
    form.emoji = def.identity.emoji || '';
    form.color = def.identity.color || '#FF5C00';
    form.archetype = def.identity.archetype || '';
    form.vibe = def.identity.vibe || '';
  }

  if (def.capabilities) {
    form.caps_memory_read = !!(
      def.capabilities.memory_read &&
      (def.capabilities.memory_read === true ||
        (Array.isArray(def.capabilities.memory_read) &&
          def.capabilities.memory_read.length))
    );
    form.caps_memory_write = !!(
      def.capabilities.memory_write &&
      (def.capabilities.memory_write === true ||
        (Array.isArray(def.capabilities.memory_write) &&
          def.capabilities.memory_write.length))
    );
    form.caps_network = !!(
      def.capabilities.network &&
      (def.capabilities.network === true ||
        (Array.isArray(def.capabilities.network) &&
          def.capabilities.network.length))
    );
    form.caps_shell = !!(
      def.capabilities.shell &&
      (def.capabilities.shell === true ||
        (Array.isArray(def.capabilities.shell) && def.capabilities.shell.length))
    );
    form.caps_agent_spawn = !!def.capabilities.agent_spawn;
  }

  return form;
}

function agentUiNormalizeList(response) {
  if (Array.isArray(response)) return response;
  if (response && Array.isArray(response.items)) return response.items;
  if (response && Array.isArray(response.agents)) return response.agents;
  if (response && Array.isArray(response.sessions)) return response.sessions;
  return [];
}

function agentUiIssueSeverityClass(severity) {
  if (!severity) return 'badge badge-dim';
  var s = String(severity).toLowerCase();
  if (s === 'error') return 'badge badge-error';
  if (s === 'warning') return 'badge badge-warn';
  return 'badge badge-dim';
}

function agentUiFormatJson(obj) {
  if (!obj) return '';
  if (typeof obj === 'string') return obj;
  try {
    return JSON.stringify(obj, null, 2);
  } catch (_) {
    return String(obj);
  }
}

// ── Page Component ──

function agentsPage() {
  return {
    // --- List ---
    agents: [],
    loading: true,
    loadError: '',
    filterState: 'all',
    searchQuery: '',

    // --- Selection ---
    selectedAgentId: '',
    selectedAgent: null,
    detailLoading: false,
    detailError: '',
    detailTab: 'info',

    // --- Runtime ---
    runtimeData: null,
    runtimeLoading: false,

    // --- Sessions ---
    sessions: [],
    sessionsLoading: false,

    // --- Skills ---
    agentSkills: [],
    allSkills: [],
    skillsLoading: false,
    newSkillId: '',

    // --- MCP Servers ---
    agentMcpServers: [],
    mcpLoading: false,
    newMcpServer: '',

    // --- Form (create/edit) ---
    showFormModal: false,
    editingId: '',
    form: agentUiDefaultForm(),
    saving: false,

    // --- Validate / Compile ---
    validating: false,
    validationResult: null,
    compiling: false,
    compileResult: null,
    compiledData: null,
    compiledTab: 'manifest',

    // --- Chat integration ---
    activeChatAgent: null,

    // --- Templates ---
    builtinTemplates: [],
    tplLoading: false,

    // --- Providers ---
    spawnProviders: [],
    spawnProvidersLoading: false,

    // --- Arky Provider Profiles ---
    arkyProfiles: [],
    arkyProfilesLoading: false,

    // --- Provider Config (detail panel) ---
    providerConfigData: null,
    providerConfigLoading: false,
    showCompiledBinding: false,
    compiledBindingData: null,
    compiledBindingLoading: false,

    // --- UI helpers ---
    emojiOptions: [
      '\u{1F916}', '\u{1F4BB}', '\u{1F50D}', '\u{270D}\uFE0F',
      '\u{1F4CA}', '\u{1F6E0}\uFE0F', '\u{1F4AC}', '\u{1F393}',
      '\u{1F310}', '\u{1F512}', '\u{26A1}', '\u{1F680}',
      '\u{1F9EA}', '\u{1F3AF}', '\u{1F4D6}', '\u{1F9D1}\u200D\u{1F4BB}',
      '\u{1F4E7}', '\u{1F3E2}', '\u{2764}\uFE0F', '\u{1F31F}',
      '\u{1F527}', '\u{1F4DD}', '\u{1F4A1}', '\u{1F3A8}'
    ],
    archetypeOptions: [
      'Assistant', 'Researcher', 'Coder', 'Writer',
      'DevOps', 'Support', 'Analyst', 'Custom'
    ],
    profileDescriptions: {
      minimal: { label: 'Minimal', desc: 'Read-only file access' },
      coding: { label: 'Coding', desc: 'Files + shell + web fetch' },
      research: { label: 'Research', desc: 'Web search + file read/write' },
      messaging: { label: 'Messaging', desc: 'Agents + memory access' },
      automation: { label: 'Automation', desc: 'All tools except custom' },
      balanced: { label: 'Balanced', desc: 'General-purpose tool set' },
      precise: { label: 'Precise', desc: 'Focused tool set for accuracy' },
      creative: { label: 'Creative', desc: 'Full tools with creative emphasis' },
      full: { label: 'Full', desc: 'All 35+ tools' }
    },

    // ═══════════════════════════════════════
    // LIFECYCLE
    // ═══════════════════════════════════════

    init() {
      var self = this;
      this.loadData();

      // If a pending agent was set, open chat inline
      var store = Alpine.store('app');
      if (store.pendingAgent) {
        this.activeChatAgent = store.pendingAgent;
      }
      this.$watch('$store.app.pendingAgent', function(agent) {
        if (agent) {
          self.activeChatAgent = agent;
        }
      });
    },

    async loadData() {
      this.loading = true;
      this.loadError = '';
      try {
        await Promise.all([
          this.loadAgents(),
          this.loadTemplates()
        ]);
      } catch (e) {
        this.loadError = e.message || 'Could not load agents. Is the daemon running?';
      }
      this.loading = false;
    },

    async loadAgents() {
      var response = await OpenFangAPI.v1.agents.list();
      this.agents = agentUiNormalizeList(response);
      // Keep global store in sync for other components
      Alpine.store('app').agents = this.agents;
    },

    async loadTemplates() {
      this.tplLoading = true;
      try {
        var results = await Promise.all([
          OpenFangAPI.get('/api/templates').catch(function() { return { templates: [] }; }),
          OpenFangAPI.get('/api/providers').catch(function() { return { providers: [] }; })
        ]);
        this.builtinTemplates = [
          {
            name: 'General Assistant',
            description: 'A versatile agent for everyday tasks and questions.',
            category: 'General',
            provider: 'groq',
            model: 'llama-3.3-70b-versatile',
            profile: 'full',
            system_prompt: 'You are a helpful, friendly assistant.'
          },
          {
            name: 'Code Helper',
            description: 'A programming-focused agent for writing and debugging code.',
            category: 'Development',
            provider: 'groq',
            model: 'llama-3.3-70b-versatile',
            profile: 'coding',
            system_prompt: 'You are an expert programmer. Help users write clean code.'
          },
          {
            name: 'Researcher',
            description: 'An analytical agent for complex topics and summaries.',
            category: 'Research',
            provider: 'groq',
            model: 'llama-3.3-70b-versatile',
            profile: 'research',
            system_prompt: 'You are a research analyst. Provide structured analysis.'
          },
          {
            name: 'Writer',
            description: 'A creative agent for drafting and editing content.',
            category: 'Writing',
            provider: 'groq',
            model: 'llama-3.3-70b-versatile',
            profile: 'full',
            system_prompt: 'You are a skilled writer and editor.'
          }
        ].concat(results[0].templates || []);
        this.spawnProviders = results[1].providers || [];
      } catch (_) {
        this.builtinTemplates = [];
      }
      this.tplLoading = false;
    },

    async loadProviders() {
      if (this.spawnProviders.length) return;
      this.spawnProvidersLoading = true;
      try {
        var resp = await OpenFangAPI.get('/api/providers');
        this.spawnProviders = resp.providers || [];
      } catch (_) {
        this.spawnProviders = [];
      }
      this.spawnProvidersLoading = false;
    },

    async loadArkyProfiles() {
      if (this.arkyProfiles.length) return;
      this.arkyProfilesLoading = true;
      try {
        var resp = await OpenFangAPI.v1.providerProfiles.list();
        this.arkyProfiles = Array.isArray(resp) ? resp : (resp.items || resp.profiles || []);
      } catch (_) {
        this.arkyProfiles = [];
      }
      this.arkyProfilesLoading = false;
    },

    arkyDriverLabel(driverId) {
      for (var i = 0; i < ARKY_DRIVERS.length; i++) {
        if (ARKY_DRIVERS[i].id === driverId) return ARKY_DRIVERS[i].label;
      }
      return driverId || '-';
    },

    arkyConfigFields() {
      return arkyDriverConfigFields(this.form.arky_driver);
    },

    onArkyDriverChange() {
      // Reset driver-specific config when driver changes
      this.form.driver_config = {};
    },

    onArkyProfileSelect() {
      var profileId = this.form.arky_profile_id;
      if (!profileId) return;
      for (var i = 0; i < this.arkyProfiles.length; i++) {
        var p = this.arkyProfiles[i];
        if (p.id === profileId) {
          if (p.driver && !this.form.arky_driver) this.form.arky_driver = p.driver;
          if (p.model && !this.form.model) this.form.model = p.model;
          if (p.defaults) {
            if (p.defaults.reasoning_effort && !this.form.reasoning_effort) {
              this.form.reasoning_effort = p.defaults.reasoning_effort;
            }
            if (p.defaults.max_tokens && !this.form.max_tokens) {
              this.form.max_tokens = String(p.defaults.max_tokens);
            }
          }
          if (p.config) {
            this.form.driver_config = Object.assign({}, p.config, this.form.driver_config);
          }
          break;
        }
      }
    },

    // Provider config for detail panel
    async loadProviderConfig() {
      if (!this.selectedAgentId) return;
      this.providerConfigLoading = true;
      this.providerConfigData = null;
      this.showCompiledBinding = false;
      this.compiledBindingData = null;
      try {
        var agent = await OpenFangAPI.v1.agents.get(this.selectedAgentId);
        var def = agent.definition || agent;
        this.providerConfigData = {
          driver: (def.provider && def.provider.driver) || (def.model && def.model.provider) || '-',
          model: (def.provider && def.provider.model) || (def.model && def.model.model) || '-',
          profile: (def.provider && def.provider.profile) || '-',
          reasoning_effort: (def.provider && def.provider.defaults && def.provider.defaults.reasoning_effort) || '-',
          max_tokens: (def.provider && def.provider.defaults && def.provider.defaults.max_tokens) || '-',
          config: (def.provider && def.provider.config) || {},
          inline_mcp_servers: (def.provider && def.provider.config && def.provider.config.mcp_servers) || null
        };
      } catch (_) {
        this.providerConfigData = null;
      }
      this.providerConfigLoading = false;
    },

    async loadCompiledBinding() {
      if (!this.selectedAgentId) return;
      this.compiledBindingLoading = true;
      try {
        this.compiledBindingData = await OpenFangAPI.v1.agents.compiled(this.selectedAgentId);
      } catch (_) {
        this.compiledBindingData = null;
      }
      this.compiledBindingLoading = false;
    },

    toggleCompiledBinding() {
      this.showCompiledBinding = !this.showCompiledBinding;
      if (this.showCompiledBinding && !this.compiledBindingData) {
        this.loadCompiledBinding();
      }
    },

    // ═══════════════════════════════════════
    // COMPUTED
    // ═══════════════════════════════════════

    get filteredAgents() {
      var self = this;
      var items = this.agents;
      if (this.filterState !== 'all') {
        items = items.filter(function(a) {
          var state = (a.state || a.runtime_status || '').toString().toLowerCase();
          return state === self.filterState;
        });
      }
      if (this.searchQuery.trim()) {
        var q = this.searchQuery.toLowerCase();
        items = items.filter(function(a) {
          return (
            ((a.name || '').toLowerCase().indexOf(q) !== -1) ||
            ((a.group || '').toLowerCase().indexOf(q) !== -1) ||
            ((a.id || '').toLowerCase().indexOf(q) !== -1) ||
            (Array.isArray(a.tags) && a.tags.join(' ').toLowerCase().indexOf(q) !== -1)
          );
        });
      }
      return items;
    },

    get runningCount() {
      return this.agents.filter(function(a) {
        return (a.state || '').toLowerCase() === 'running';
      }).length;
    },

    get stoppedCount() {
      return this.agents.filter(function(a) {
        return (a.state || '').toLowerCase() !== 'running';
      }).length;
    },

    get validationIssues() {
      if (
        !this.validationResult ||
        !Array.isArray(this.validationResult.issues)
      ) {
        return [];
      }
      return this.validationResult.issues;
    },

    get compileSummary() {
      if (!this.compileResult || !this.compileResult.compiled) return null;
      var compiled = this.compileResult.compiled;
      return {
        manifest: compiled.manifest || compiled.agent_manifest || null,
        binding: compiled.binding || compiled.provider_binding || null,
        metadata: compiled.metadata || compiled
      };
    },

    // ═══════════════════════════════════════
    // SELECTION
    // ═══════════════════════════════════════

    async selectAgent(id) {
      if (!id) {
        this.clearSelection();
        return;
      }
      if (this.selectedAgentId === id && this.selectedAgent) return;

      this.selectedAgentId = id;
      this.detailLoading = true;
      this.detailError = '';
      this.detailTab = 'info';
      this.validationResult = null;
      this.compileResult = null;
      this.compiledData = null;
      this.runtimeData = null;
      this.sessions = [];
      this.agentSkills = [];
      this.agentMcpServers = [];

      try {
        var responses = await Promise.all([
          OpenFangAPI.v1.agents.get(id),
          OpenFangAPI.v1.agents.runtime(id).catch(function() { return null; })
        ]);
        this.selectedAgent = responses[0];
        this.runtimeData = responses[1];
        this.form = agentUiHydrateForm(this.selectedAgent);
      } catch (e) {
        this.detailError = e.message || 'Could not load agent detail.';
      }

      this.detailLoading = false;
    },

    clearSelection() {
      this.selectedAgentId = '';
      this.selectedAgent = null;
      this.detailError = '';
      this.detailTab = 'info';
      this.validationResult = null;
      this.compileResult = null;
      this.compiledData = null;
      this.runtimeData = null;
      this.sessions = [];
      this.agentSkills = [];
      this.agentMcpServers = [];
    },

    async refreshSelected() {
      if (!this.selectedAgentId) return;
      try {
        this.selectedAgent = await OpenFangAPI.v1.agents.get(this.selectedAgentId);
        this.form = agentUiHydrateForm(this.selectedAgent);
      } catch (_) { /* keep existing data */ }
    },

    // ═══════════════════════════════════════
    // AGENT NAME / ID HELPERS
    // ═══════════════════════════════════════

    agentName(agent) {
      if (!agent) return '';
      return agent.name || (agent.definition && agent.definition.name) || agent.id || '';
    },

    agentProvider(agent) {
      if (!agent) return '';
      if (agent.model_provider) return agent.model_provider;
      if (agent.definition && agent.definition.model) return agent.definition.model.provider || '';
      return '';
    },

    agentModel(agent) {
      if (!agent) return '';
      if (agent.model_name) return agent.model_name;
      if (agent.definition && agent.definition.model) return agent.definition.model.model || '';
      return '';
    },

    agentState(agent) {
      if (!agent) return 'stopped';
      if (agent.state) return agent.state;
      if (agent.runtime_status && agent.runtime_status.state) {
        return agent.runtime_status.state;
      }
      return 'stopped';
    },

    agentEnabled(agent) {
      if (!agent) return false;
      if (typeof agent.enabled === 'boolean') return agent.enabled;
      if (agent.definition && typeof agent.definition.enabled === 'boolean') {
        return agent.definition.enabled;
      }
      return true;
    },

    agentGroup(agent) {
      if (!agent) return '';
      return agent.group || (agent.definition && agent.definition.group) || '';
    },

    agentTags(agent) {
      if (!agent) return [];
      if (Array.isArray(agent.tags)) return agent.tags;
      if (agent.definition && Array.isArray(agent.definition.tags)) {
        return agent.definition.tags;
      }
      return [];
    },

    agentOriginKind(agent) {
      if (!agent) return '';
      if (agent.origin && agent.origin.kind) return agent.origin.kind;
      return '';
    },

    // ═══════════════════════════════════════
    // CRUD — CREATE / EDIT
    // ═══════════════════════════════════════

    openCreateForm() {
      this.editingId = '';
      this.form = agentUiDefaultForm();
      this.validationResult = null;
      this.compileResult = null;
      this.showFormModal = true;
      this.loadProviders();
      this.loadArkyProfiles();
    },

    openEditForm() {
      if (!this.selectedAgent) return;
      this.editingId = this.selectedAgentId;
      this.form = agentUiHydrateForm(this.selectedAgent);
      this.validationResult = null;
      this.compileResult = null;
      this.showFormModal = true;
      this.loadProviders();
      this.loadArkyProfiles();
    },

    async saveAgent() {
      if (!(this.form.name || '').trim()) {
        OpenFangToast.warn('Please enter an agent name');
        return;
      }
      this.saving = true;
      try {
        var definition = agentUiBuildDefinition(this.form);
        var result;
        if (this.editingId) {
          result = await OpenFangAPI.v1.agents.update(this.editingId, definition);
          OpenFangToast.success('Agent updated: ' + definition.name);
        } else {
          result = await OpenFangAPI.v1.agents.create(definition);
          OpenFangToast.success('Agent created: ' + definition.name);
        }
        this.showFormModal = false;
        await this.loadAgents();
        var newId = this.editingId || (result && (result.id || result.agent_id)) || '';
        if (newId) {
          await this.selectAgent(newId);
        }
      } catch (e) {
        OpenFangToast.error('Failed to save agent: ' + e.message);
      }
      this.saving = false;
    },

    // ═══════════════════════════════════════
    // CRUD — DELETE
    // ═══════════════════════════════════════

    deleteAgent() {
      if (!this.selectedAgentId) return;
      var self = this;
      var name = this.agentName(this.selectedAgent);

      OpenFangToast.confirm(
        'Delete Agent',
        'Delete agent "' + name + '"? This cannot be undone.',
        async function() {
          try {
            await OpenFangAPI.v1.agents.delete(self.selectedAgentId);
            OpenFangToast.success('Agent "' + name + '" deleted');
            self.clearSelection();
            await self.loadAgents();
          } catch (e) {
            OpenFangToast.error('Failed to delete agent: ' + e.message);
          }
        }
      );
    },

    // ═══════════════════════════════════════
    // VALIDATE / COMPILE
    // ═══════════════════════════════════════

    async validateAgent() {
      this.validating = true;
      this.validationResult = null;
      try {
        var definition = agentUiBuildDefinition(this.form);
        this.validationResult = await OpenFangAPI.v1.agents.validate({
          definition: definition
        });
        if (this.validationResult.valid) {
          OpenFangToast.success('Agent definition is valid');
        } else {
          OpenFangToast.warn('Agent definition has validation issues');
        }
      } catch (e) {
        OpenFangToast.error('Validation failed: ' + e.message);
      }
      this.validating = false;
    },

    async compileAgent() {
      this.compiling = true;
      this.compileResult = null;
      try {
        var definition = agentUiBuildDefinition(this.form);
        this.compileResult = await OpenFangAPI.v1.agents.compile({
          definition: definition
        });
        OpenFangToast.success('Agent compiled successfully');
      } catch (e) {
        OpenFangToast.error('Compilation failed: ' + e.message);
      }
      this.compiling = false;
    },

    async loadCompiled() {
      if (!this.selectedAgentId) return;
      this.compiledData = null;
      try {
        this.compiledData = await OpenFangAPI.v1.agents.compiled(
          this.selectedAgentId
        );
      } catch (_) {
        this.compiledData = null;
      }
    },

    issuesForPath(prefix) {
      return this.validationIssues.filter(function(issue) {
        return (issue.path || '').indexOf(prefix) === 0;
      });
    },

    issueSeverityClass(issue) {
      return agentUiIssueSeverityClass(issue && issue.severity);
    },

    // ═══════════════════════════════════════
    // RUNTIME
    // ═══════════════════════════════════════

    async loadRuntime() {
      if (!this.selectedAgentId) return;
      this.runtimeLoading = true;
      try {
        this.runtimeData = await OpenFangAPI.v1.agents.runtime(
          this.selectedAgentId
        );
      } catch (_) {
        this.runtimeData = null;
      }
      this.runtimeLoading = false;
    },

    async startAgent() {
      if (!this.selectedAgentId) return;
      try {
        await OpenFangAPI.v1.agents.startRuntime(this.selectedAgentId);
        OpenFangToast.success('Agent started');
        await Promise.all([this.loadRuntime(), this.loadAgents()]);
      } catch (e) {
        OpenFangToast.error('Failed to start agent: ' + e.message);
      }
    },

    stopAgentRuntime() {
      if (!this.selectedAgentId) return;
      var self = this;
      OpenFangToast.confirm(
        'Stop Agent',
        'Stop this agent? Active sessions will be interrupted.',
        async function() {
          try {
            await OpenFangAPI.v1.agents.stopRuntime(self.selectedAgentId);
            OpenFangToast.success('Agent stopped');
            await Promise.all([self.loadRuntime(), self.loadAgents()]);
          } catch (e) {
            OpenFangToast.error('Failed to stop agent: ' + e.message);
          }
        }
      );
    },

    restartAgent() {
      if (!this.selectedAgentId) return;
      var self = this;
      OpenFangToast.confirm(
        'Restart Agent',
        'Restart this agent? Active sessions will be restarted.',
        async function() {
          try {
            await OpenFangAPI.v1.agents.restartRuntime(self.selectedAgentId);
            OpenFangToast.success('Agent restarted');
            await Promise.all([self.loadRuntime(), self.loadAgents()]);
          } catch (e) {
            OpenFangToast.error('Failed to restart agent: ' + e.message);
          }
        }
      );
    },

    async setAgentMode(mode) {
      if (!this.selectedAgentId) return;
      try {
        await OpenFangAPI.v1.agents.setMode(this.selectedAgentId, {
          mode: mode
        });
        OpenFangToast.success('Mode set to ' + mode);
        await Promise.all([this.loadRuntime(), this.loadAgents()]);
      } catch (e) {
        OpenFangToast.error('Failed to set mode: ' + e.message);
      }
    },

    runtimeStateLabel() {
      if (!this.runtimeData) return 'Unknown';
      return this.runtimeData.state || this.runtimeData.status || 'Unknown';
    },

    runtimeHealthy() {
      if (!this.runtimeData) return false;
      return this.runtimeData.healthy === true;
    },

    runtimeMode() {
      if (!this.runtimeData) return '';
      return this.runtimeData.mode || '';
    },

    runtimeActiveSessions() {
      if (!this.runtimeData) return 0;
      return this.runtimeData.active_sessions || 0;
    },

    runtimeActiveDispatches() {
      if (!this.runtimeData) return 0;
      return this.runtimeData.active_dispatches || 0;
    },

    // ═══════════════════════════════════════
    // SESSIONS
    // ═══════════════════════════════════════

    async loadSessions() {
      if (!this.selectedAgentId) return;
      this.sessionsLoading = true;
      try {
        var resp = await OpenFangAPI.v1.agents.sessions(this.selectedAgentId);
        this.sessions = agentUiNormalizeList(resp);
      } catch (_) {
        this.sessions = [];
      }
      this.sessionsLoading = false;
    },

    async createNewSession() {
      if (!this.selectedAgentId) return;
      try {
        await OpenFangAPI.v1.agents.createSession(this.selectedAgentId);
        OpenFangToast.success('Session created');
        await this.loadSessions();
      } catch (e) {
        OpenFangToast.error('Failed to create session: ' + e.message);
      }
    },

    async activateSession(sid) {
      if (!this.selectedAgentId) return;
      try {
        await OpenFangAPI.v1.agents.activateSession(
          this.selectedAgentId, sid
        );
        OpenFangToast.success('Session activated');
        await this.loadSessions();
      } catch (e) {
        OpenFangToast.error('Failed to activate session: ' + e.message);
      }
    },

    resetSession(sid) {
      var self = this;
      OpenFangToast.confirm(
        'Reset Session',
        'Reset this session? All messages will be cleared.',
        async function() {
          try {
            await OpenFangAPI.v1.agents.resetSession(
              self.selectedAgentId, sid
            );
            OpenFangToast.success('Session reset');
            await self.loadSessions();
          } catch (e) {
            OpenFangToast.error('Failed to reset session: ' + e.message);
          }
        }
      );
    },

    async compactSession(sid) {
      if (!this.selectedAgentId) return;
      try {
        await OpenFangAPI.v1.agents.compactSession(
          this.selectedAgentId, sid
        );
        OpenFangToast.success('Session compacted');
        await this.loadSessions();
      } catch (e) {
        OpenFangToast.error('Failed to compact session: ' + e.message);
      }
    },

    // ═══════════════════════════════════════
    // SKILLS ASSIGNMENT
    // ═══════════════════════════════════════

    async loadSkills() {
      if (!this.selectedAgentId) return;
      this.skillsLoading = true;
      try {
        var resp = await OpenFangAPI.get(
          '/api/agents/' + this.selectedAgentId + '/skills'
        );
        this.agentSkills = Array.isArray(resp)
          ? resp
          : (resp.skills || []);
      } catch (_) {
        this.agentSkills = [];
      }
      try {
        var allResp = await OpenFangAPI.v1.skills.list();
        this.allSkills = agentUiNormalizeList(allResp);
      } catch (_) {
        this.allSkills = [];
      }
      this.skillsLoading = false;
    },

    async saveSkills() {
      if (!this.selectedAgentId) return;
      try {
        await OpenFangAPI.put(
          '/api/agents/' + this.selectedAgentId + '/skills',
          { skills: this.agentSkills }
        );
        OpenFangToast.success('Skills updated');
      } catch (e) {
        OpenFangToast.error('Failed to update skills: ' + e.message);
      }
    },

    addSkill() {
      var val = (this.newSkillId || '').trim();
      if (!val) return;
      if (this.agentSkills.indexOf(val) === -1) {
        this.agentSkills.push(val);
        this.saveSkills();
      }
      this.newSkillId = '';
    },

    removeSkill(skill) {
      this.agentSkills = this.agentSkills.filter(function(s) {
        return s !== skill;
      });
      this.saveSkills();
    },

    // ═══════════════════════════════════════
    // MCP SERVERS ASSIGNMENT
    // ═══════════════════════════════════════

    async loadMcpServers() {
      if (!this.selectedAgentId) return;
      this.mcpLoading = true;
      try {
        var resp = await OpenFangAPI.get(
          '/api/agents/' + this.selectedAgentId + '/mcp_servers'
        );
        this.agentMcpServers = Array.isArray(resp)
          ? resp
          : (resp.mcp_servers || []);
      } catch (_) {
        this.agentMcpServers = [];
      }
      this.mcpLoading = false;
    },

    async saveMcpServers() {
      if (!this.selectedAgentId) return;
      try {
        await OpenFangAPI.put(
          '/api/agents/' + this.selectedAgentId + '/mcp_servers',
          { mcp_servers: this.agentMcpServers }
        );
        OpenFangToast.success('MCP servers updated');
      } catch (e) {
        OpenFangToast.error('Failed to update MCP servers: ' + e.message);
      }
    },

    addMcpServer() {
      var val = (this.newMcpServer || '').trim();
      if (!val) return;
      if (this.agentMcpServers.indexOf(val) === -1) {
        this.agentMcpServers.push(val);
        this.saveMcpServers();
      }
      this.newMcpServer = '';
    },

    removeMcpServer(server) {
      this.agentMcpServers = this.agentMcpServers.filter(function(s) {
        return s !== server;
      });
      this.saveMcpServers();
    },

    // ═══════════════════════════════════════
    // CHAT INTEGRATION
    // ═══════════════════════════════════════

    chatWithAgent(agent) {
      Alpine.store('app').pendingAgent = agent;
      this.activeChatAgent = agent;
    },

    closeChat() {
      this.activeChatAgent = null;
      OpenFangAPI.wsDisconnect();
    },

    // ═══════════════════════════════════════
    // QUICK-SPAWN FROM TEMPLATE
    // ═══════════════════════════════════════

    async spawnFromTemplate(template) {
      try {
        var definition = {
          name: template.name,
          description: template.description || '',
          module: 'builtin:chat',
          enabled: true,
          profile: template.profile || 'full',
          model: {
            provider: template.provider || 'groq',
            model: template.model || 'llama-3.3-70b-versatile',
            system_prompt: template.system_prompt || ''
          }
        };
        var result = await OpenFangAPI.v1.agents.create(definition);
        var agentId = result && (result.id || result.agent_id);
        OpenFangToast.success('Agent "' + template.name + '" created');
        await this.loadAgents();
        if (agentId) {
          this.chatWithAgent({
            id: agentId,
            name: template.name,
            model_provider: template.provider,
            model_name: template.model
          });
        }
      } catch (e) {
        OpenFangToast.error('Failed to create agent: ' + e.message);
      }
    },

    // ═══════════════════════════════════════
    // DISPLAY HELPERS
    // ═══════════════════════════════════════

    profileInfo(name) {
      return this.profileDescriptions[name] || { label: name, desc: '' };
    },

    formatJson(obj) {
      return agentUiFormatJson(obj);
    },

    stateClass(state) {
      if (!state) return 'badge badge-dim';
      var s = String(state).toLowerCase();
      if (s === 'running') return 'badge badge-success';
      if (s === 'stopped' || s === 'idle') return 'badge badge-dim';
      if (s === 'error' || s === 'failed') return 'badge badge-error';
      if (s === 'starting' || s === 'loading') return 'badge badge-warn';
      return 'badge badge-dim';
    }
  };
}
