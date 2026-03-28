// OpenFang API v1 Client — domain-specific wrappers over OpenFangAPI transport
'use strict';

(function() {
  // Helper: build query string from a params object, omitting null/undefined values
  function qs(params) {
    if (!params) return '';
    var parts = [];
    var keys = Object.keys(params);
    for (var i = 0; i < keys.length; i++) {
      var k = keys[i];
      var v = params[k];
      if (v !== null && v !== undefined && v !== '') {
        parts.push(encodeURIComponent(k) + '=' + encodeURIComponent(v));
      }
    }
    return parts.length ? '?' + parts.join('&') : '';
  }

  // Helper: unwrap v1 error envelope {error: {code, message, details}}
  function unwrap(promise) {
    return promise.catch(function(err) {
      // The base client already extracts error text, but v1 may wrap further
      if (err && err.message) {
        try {
          var parsed = JSON.parse(err.message);
          if (parsed && parsed.error && parsed.error.message) {
            throw new Error(parsed.error.message);
          }
        } catch (e) {
          if (e !== err) throw err;
        }
      }
      throw err;
    });
  }

  OpenFangAPI.v1 = {
    // ── Runs ──
    runs: {
      list: function(params) {
        return unwrap(OpenFangAPI.get('/api/v1/runs' + qs(params)));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/runs/' + id));
      },
      pause: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/runs/' + id + '/pause'));
      },
      resume: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/runs/' + id + '/resume'));
      },
      cancel: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/runs/' + id + '/cancel'));
      },
      checkpoints: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/runs/' + id + '/checkpoints'));
      },
      signals: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/runs/' + id + '/signals'));
      },
      sendSignal: function(id, body) {
        return unwrap(OpenFangAPI.post('/api/v1/runs/' + id + '/signals', body));
      },
      dispatches: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/runs/' + id + '/dispatches'));
      },
      hitlRequests: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/runs/' + id + '/hitl-requests'));
      }
    },

    // ── HITL Requests ──
    hitl: {
      list: function(params) {
        return unwrap(OpenFangAPI.get('/api/v1/hitl-requests' + qs(params)));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/hitl-requests/' + id));
      },
      answer: function(id, body) {
        return unwrap(OpenFangAPI.post('/api/v1/hitl-requests/' + id + '/answer', body));
      },
      cancel: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/hitl-requests/' + id + '/cancel'));
      }
    },

    // ── Dispatches ──
    dispatches: {
      list: function(params) {
        return unwrap(OpenFangAPI.get('/api/v1/dispatches' + qs(params)));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/dispatches/' + id));
      },
      children: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/dispatches/' + id + '/children'));
      },
      retry: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/dispatches/' + id + '/retry'));
      },
      cancel: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/dispatches/' + id + '/cancel'));
      }
    },

    // ── Tasks ──
    tasks: {
      list: function(params) {
        return unwrap(OpenFangAPI.get('/api/v1/tasks' + qs(params)));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/tasks/' + id));
      },
      create: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/tasks', body));
      },
      update: function(id, body) {
        return unwrap(OpenFangAPI.put('/api/v1/tasks/' + id, body));
      },
      delete: function(id) {
        return unwrap(OpenFangAPI.del('/api/v1/tasks/' + id));
      },
      subtasks: function(id, params) {
        return unwrap(OpenFangAPI.get('/api/v1/tasks/' + id + '/subtasks' + qs(params)));
      },
      replan: function(id, body) {
        return unwrap(OpenFangAPI.post('/api/v1/tasks/' + id + '/replan', body));
      },
      artifacts: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/tasks/' + id + '/artifacts'));
      },
      docs: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/tasks/' + id + '/docs'));
      },
      files: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/tasks/' + id + '/files'));
      }
    },

    // ── Subtasks ──
    subtasks: {
      list: function(params) {
        return unwrap(OpenFangAPI.get('/api/v1/subtasks' + qs(params)));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/subtasks/' + id));
      },
      update: function(id, body) {
        return unwrap(OpenFangAPI.put('/api/v1/subtasks/' + id, body));
      },
      delete: function(id) {
        return unwrap(OpenFangAPI.del('/api/v1/subtasks/' + id));
      }
    },

    // ── Workflows ──
    workflows: {
      list: function() {
        return unwrap(OpenFangAPI.get('/api/v1/workflows'));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/workflows/' + id));
      },
      create: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/workflows', body));
      },
      update: function(id, body) {
        return unwrap(OpenFangAPI.put('/api/v1/workflows/' + id, body));
      },
      delete: function(id) {
        return unwrap(OpenFangAPI.del('/api/v1/workflows/' + id));
      },
      validate: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/workflows/validate', body));
      },
      compile: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/workflows/compile', body));
      },
      compiled: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/workflows/' + id + '/compiled'));
      },
      fork: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/workflows/' + id + '/fork'));
      },
      runtime: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/workflows/' + id + '/runtime'));
      },
      runs: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/workflows/' + id + '/runs'));
      },
      startRun: function(id, body) {
        return unwrap(OpenFangAPI.post('/api/v1/workflows/' + id + '/runs', body));
      },
      dryRun: function(id, body) {
        return unwrap(
          OpenFangAPI.post('/api/v1/workflows/' + id + '/runs/dry-run', body)
        );
      }
    },

    // ── Triggers ──
    triggers: {
      list: function() {
        return unwrap(OpenFangAPI.get('/api/v1/triggers'));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/triggers/' + id));
      },
      create: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/triggers', body));
      },
      update: function(id, body) {
        return unwrap(OpenFangAPI.put('/api/v1/triggers/' + id, body));
      },
      delete: function(id) {
        return unwrap(OpenFangAPI.del('/api/v1/triggers/' + id));
      },
      validate: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/triggers/validate', body));
      },
      compile: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/triggers/compile', body));
      },
      compiled: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/triggers/' + id + '/compiled'));
      },
      fork: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/triggers/' + id + '/fork'));
      },
      runtime: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/triggers/' + id + '/runtime'));
      },
      enable: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/triggers/' + id + '/enable'));
      },
      disable: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/triggers/' + id + '/disable'));
      },
      test: function(id, event) {
        return unwrap(OpenFangAPI.post('/api/v1/triggers/' + id + '/test', event));
      }
    },

    // ── Schedules ──
    schedules: {
      list: function() {
        return unwrap(OpenFangAPI.get('/api/v1/schedules'));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/schedules/' + id));
      },
      create: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/schedules', body));
      },
      update: function(id, body) {
        return unwrap(OpenFangAPI.put('/api/v1/schedules/' + id, body));
      },
      delete: function(id) {
        return unwrap(OpenFangAPI.del('/api/v1/schedules/' + id));
      },
      validate: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/schedules/validate', body));
      },
      fork: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/schedules/' + id + '/fork'));
      },
      runtime: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/schedules/' + id + '/runtime'));
      },
      enable: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/schedules/' + id + '/enable'));
      },
      disable: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/schedules/' + id + '/disable'));
      },
      runNow: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/schedules/' + id + '/run-now'));
      },
      dryRun: function(id) {
        return unwrap(
          OpenFangAPI.post('/api/v1/schedules/' + id + '/run-now/dry-run')
        );
      }
    },

    // ── Agents ──
    agents: {
      list: function() {
        return unwrap(OpenFangAPI.get('/api/v1/agents'));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/agents/' + id));
      },
      create: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/agents', body));
      },
      update: function(id, body) {
        return unwrap(OpenFangAPI.put('/api/v1/agents/' + id, body));
      },
      delete: function(id) {
        return unwrap(OpenFangAPI.del('/api/v1/agents/' + id));
      },
      validate: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/agents/validate', body));
      },
      compile: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/agents/compile', body));
      },
      compiled: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/agents/' + id + '/compiled'));
      },
      runtime: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/agents/' + id + '/runtime'));
      },
      startRuntime: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/agents/' + id + '/runtime/start'));
      },
      stopRuntime: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/agents/' + id + '/runtime/stop'));
      },
      restartRuntime: function(id) {
        return unwrap(
          OpenFangAPI.post('/api/v1/agents/' + id + '/runtime/restart')
        );
      },
      setMode: function(id, body) {
        return unwrap(
          OpenFangAPI.put('/api/v1/agents/' + id + '/runtime/mode', body)
        );
      },
      sessions: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/agents/' + id + '/sessions'));
      },
      createSession: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/agents/' + id + '/sessions'));
      },
      activateSession: function(id, sid) {
        return unwrap(
          OpenFangAPI.post(
            '/api/v1/agents/' + id + '/sessions/' + sid + '/activate'
          )
        );
      },
      resetSession: function(id, sid) {
        return unwrap(
          OpenFangAPI.post(
            '/api/v1/agents/' + id + '/sessions/' + sid + '/reset'
          )
        );
      },
      compactSession: function(id, sid) {
        return unwrap(
          OpenFangAPI.post(
            '/api/v1/agents/' + id + '/sessions/' + sid + '/compact'
          )
        );
      },
      sendMessage: function(id, body) {
        return unwrap(OpenFangAPI.post('/api/v1/agents/' + id + '/messages', body));
      },
      dryRunMessage: function(id, body) {
        return unwrap(
          OpenFangAPI.post('/api/v1/agents/' + id + '/messages/dry-run', body)
        );
      }
    },

    // ── Events ──
    events: {
      send: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/events', body));
      },
      dryRun: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/events/dry-run', body));
      }
    },

    // ── Looper Runs ──
    looper: {
      list: function(params) {
        return unwrap(OpenFangAPI.get('/api/v1/looper-runs' + qs(params)));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/looper-runs/' + id));
      },
      create: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/looper-runs', body));
      },
      subtasks: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/looper-runs/' + id + '/subtasks'));
      },
      pause: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/looper-runs/' + id + '/pause'));
      },
      resume: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/looper-runs/' + id + '/resume'));
      },
      cancel: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/looper-runs/' + id + '/cancel'));
      }
    },

    // ── Packs ──
    packs: {
      list: function() {
        return unwrap(OpenFangAPI.get('/api/v1/packs'));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/packs/' + id));
      },
      objects: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/packs/' + id + '/objects'));
      },
      install: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/packs/install', body));
      },
      upgrade: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/packs/' + id + '/upgrade'));
      },
      upgradeDryRun: function(id) {
        return unwrap(
          OpenFangAPI.post('/api/v1/packs/' + id + '/upgrade/dry-run')
        );
      },
      uninstall: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/packs/' + id + '/uninstall'));
      },
      fork: function(id) {
        return unwrap(OpenFangAPI.post('/api/v1/packs/' + id + '/fork'));
      }
    },

    // ── Artifacts ──
    artifacts: {
      list: function(params) {
        return unwrap(OpenFangAPI.get('/api/v1/artifacts' + qs(params)));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/artifacts/' + id));
      },
      versions: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/artifacts/' + id + '/versions'));
      }
    },

    // ── Docs ──
    docs: {
      list: function(params) {
        return unwrap(OpenFangAPI.get('/api/v1/docs' + qs(params)));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/docs/' + id));
      },
      versions: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/docs/' + id + '/versions'));
      }
    },

    // ── Skills ──
    skills: {
      list: function(params) {
        return unwrap(OpenFangAPI.get('/api/v1/skills' + qs(params)));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/skills/' + id));
      }
    },

    // ── Provider Profiles ──
    providerProfiles: {
      list: function() {
        return unwrap(OpenFangAPI.get('/api/v1/provider-profiles'));
      },
      get: function(id) {
        return unwrap(OpenFangAPI.get('/api/v1/provider-profiles/' + id));
      },
      create: function(body) {
        return unwrap(OpenFangAPI.post('/api/v1/provider-profiles', body));
      },
      update: function(id, body) {
        return unwrap(
          OpenFangAPI.put('/api/v1/provider-profiles/' + id, body)
        );
      },
      delete: function(id) {
        return unwrap(OpenFangAPI.del('/api/v1/provider-profiles/' + id));
      }
    }
  };
})();
