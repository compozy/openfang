// OpenFang SSE Client — shared EventSource wrapper with reconnect, auth, event routing
'use strict';

var OpenFangSSE = (function() {
  var MAX_RECONNECT = 5;
  var BASE_DELAY = 1000;
  var MAX_DELAY = 10000;

  /**
   * Connect to an SSE endpoint.
   *
   * @param {string} path - Endpoint path, e.g. '/api/v1/hitl-requests/stream'
   * @param {Object} handlers - Map of event type to callback: { 'hitl.created': fn(data) }
   *   The special key 'message' handles unnamed events (the default SSE event type).
   * @param {Object} [options]
   * @param {boolean} [options.reconnect=true] - Auto-reconnect on error
   * @param {string|null} [options.lastEventId=null] - Resume from this event ID
   * @param {string|null} [options.token=null] - Override auth token (defaults to OpenFangAPI.getToken())
   * @returns {{ close: Function, isConnected: Function }}
   */
  function connect(path, handlers, options) {
    options = options || {};
    var reconnect = options.reconnect !== false;
    var lastEventId = options.lastEventId || null;
    var token = options.token || null;
    var attempt = 0;
    var es = null;
    var closed = false;
    var connected = false;
    var reconnectTimer = null;

    function buildUrl() {
      var url = window.location.origin + path;
      var params = [];

      // Inject auth token
      var tok = token || (typeof OpenFangAPI !== 'undefined' ? OpenFangAPI.getToken() : '');
      if (tok) {
        params.push('token=' + encodeURIComponent(tok));
      }

      // Append params — if path already has ?, use &
      if (params.length) {
        url += (path.indexOf('?') >= 0 ? '&' : '?') + params.join('&');
      }
      return url;
    }

    function open() {
      if (closed) return;

      var url = buildUrl();

      // Pass lastEventId via EventSource init if supported, plus header
      var initOpts = {};
      if (lastEventId) {
        // Standard way: browsers send Last-Event-ID automatically if the server
        // sent an `id:` field. We also pass it as a query param for servers that
        // don't read the header.
        url += (url.indexOf('?') >= 0 ? '&' : '?')
          + 'last_event_id=' + encodeURIComponent(lastEventId);
      }

      es = new EventSource(url);

      es.onopen = function() {
        connected = true;
        attempt = 0;
        updateConnectionState('connected');
      };

      // Handle named event types from handlers map
      var eventTypes = Object.keys(handlers || {});
      for (var i = 0; i < eventTypes.length; i++) {
        (function(type) {
          if (type === 'message') {
            // Default event type — handled by es.onmessage
            return;
          }
          es.addEventListener(type, function(e) {
            trackEventId(e);
            var data = parseData(e);
            if (handlers[type]) handlers[type](data, e);
          });
        })(eventTypes[i]);
      }

      // Default message handler (unnamed events)
      es.onmessage = function(e) {
        trackEventId(e);
        var data = parseData(e);
        if (handlers && handlers.message) handlers.message(data, e);
      };

      es.onerror = function() {
        connected = false;

        if (closed) return;

        // Close the errored source so the browser doesn't auto-reconnect
        if (es) {
          es.close();
          es = null;
        }

        if (reconnect && attempt < MAX_RECONNECT) {
          attempt++;
          updateConnectionState('reconnecting');
          var delay = Math.min(BASE_DELAY * Math.pow(2, attempt - 1), MAX_DELAY);
          reconnectTimer = setTimeout(open, delay);
        } else if (attempt >= MAX_RECONNECT) {
          updateConnectionState('disconnected');
        }
      };
    }

    function trackEventId(e) {
      if (e && e.lastEventId) {
        lastEventId = e.lastEventId;
      }
    }

    function parseData(e) {
      if (!e || !e.data) return null;
      try {
        return JSON.parse(e.data);
      } catch (err) {
        return e.data;
      }
    }

    function updateConnectionState(state) {
      // Integrate with Alpine store if available
      if (typeof Alpine !== 'undefined') {
        try {
          var store = Alpine.store('app');
          if (store && store.connectionState !== undefined) {
            store.connectionState = state;
          }
        } catch (e) {
          // Alpine not ready yet — ignore
        }
      }
    }

    function close() {
      closed = true;
      connected = false;
      if (reconnectTimer) {
        clearTimeout(reconnectTimer);
        reconnectTimer = null;
      }
      if (es) {
        es.close();
        es = null;
      }
    }

    function isConnected() {
      return connected;
    }

    // Start the connection
    open();

    return {
      close: close,
      isConnected: isConnected
    };
  }

  return {
    connect: connect
  };
})();
