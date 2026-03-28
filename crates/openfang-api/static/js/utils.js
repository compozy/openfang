// OpenFang Shared Utilities — single-source replacements for duplicated helpers
'use strict';

var OpenFangUtils = (function() {
  /**
   * Relative time string: "just now", "5m ago", "2h ago", "3d ago".
   * Handles both past and future timestamps.
   */
  function timeAgo(dateStr) {
    if (!dateStr) return '';
    var ts = new Date(dateStr).getTime();
    if (isNaN(ts)) return '';
    var diff = Math.floor((Date.now() - ts) / 1000);

    if (diff < 0) {
      // Future timestamp
      var abs = Math.abs(diff);
      if (abs < 60) return 'in ' + abs + 's';
      if (abs < 3600) return 'in ' + Math.floor(abs / 60) + 'm';
      if (abs < 86400) return 'in ' + Math.floor(abs / 3600) + 'h';
      return 'in ' + Math.floor(abs / 86400) + 'd';
    }

    if (diff < 10) return 'just now';
    if (diff < 60) return diff + 's ago';
    if (diff < 3600) return Math.floor(diff / 60) + 'm ago';
    if (diff < 86400) return Math.floor(diff / 3600) + 'h ago';
    return Math.floor(diff / 86400) + 'd ago';
  }

  /**
   * Format a date string as "Mar 27, 2026".
   */
  function formatDate(dateStr) {
    if (!dateStr) return '';
    var d = new Date(dateStr);
    if (isNaN(d.getTime())) return '';
    var months = [
      'Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun',
      'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'
    ];
    return months[d.getMonth()] + ' ' + d.getDate() + ', ' + d.getFullYear();
  }

  /**
   * Format a date string as "Mar 27, 2026 3:45 PM".
   */
  function formatDateTime(dateStr) {
    if (!dateStr) return '';
    var d = new Date(dateStr);
    if (isNaN(d.getTime())) return '';
    var h = d.getHours();
    var m = d.getMinutes();
    var ampm = h >= 12 ? 'PM' : 'AM';
    h = h % 12 || 12;
    var mStr = m < 10 ? '0' + m : '' + m;
    return formatDate(dateStr) + ' ' + h + ':' + mStr + ' ' + ampm;
  }

  /**
   * Map a status string to a CSS badge class.
   * Returns the full class string, e.g. "badge badge-success".
   */
  function statusBadge(status) {
    if (!status) return 'badge badge-dim';
    var s = String(status).toLowerCase();

    if (s === 'pending') return 'badge badge-run-pending';
    if (s === 'running') return 'badge badge-run-running';
    if (s === 'waiting_signal') return 'badge badge-run-waiting-signal';
    if (s === 'waiting_hitl') return 'badge badge-run-waiting-hitl';
    if (s === 'paused') return 'badge badge-run-paused';
    if (s === 'completed') return 'badge badge-run-completed';
    if (s === 'failed' || s === 'timed_out') return 'badge badge-run-failed';
    if (s === 'cancelled') return 'badge badge-run-cancelled';

    // Success states
    if (s === 'active' || s === 'enabled' || s === 'online') {
      return 'badge badge-success';
    }
    if (s === 'done' || s === 'answered' || s === 'installed') {
      return 'badge badge-success';
    }

    // Error states
    if (s === 'error' || s === 'rejected' || s === 'dead') {
      return 'badge badge-error';
    }

    // Warning states
    if (s === 'waiting' || s === 'suspended') {
      return 'badge badge-warn';
    }

    // Info states
    if (s === 'created' || s === 'queued' || s === 'ready' || s === 'scheduled') {
      return 'badge badge-created';
    }

    // Cancelled / inactive
    if (s === 'disabled' || s === 'stopped' || s === 'offline') {
      return 'badge badge-dim';
    }

    return 'badge badge-dim';
  }

  /**
   * Show a styled confirmation modal via OpenFangToast.confirm().
   */
  function confirmAction(title, message, onConfirm) {
    if (typeof OpenFangToast !== 'undefined' && OpenFangToast.confirm) {
      OpenFangToast.confirm(title, message, onConfirm);
    }
  }

  /**
   * Truncate a string to maxLen characters, appending ellipsis if truncated.
   */
  function truncate(str, maxLen) {
    if (!str) return '';
    maxLen = maxLen || 80;
    if (str.length <= maxLen) return str;
    return str.slice(0, maxLen) + '\u2026';
  }

  /**
   * Copy text to the clipboard. Shows a toast on success/failure.
   */
  function copyToClipboard(text) {
    if (!navigator.clipboard) {
      // Fallback for insecure contexts
      var ta = document.createElement('textarea');
      ta.value = text;
      ta.style.position = 'fixed';
      ta.style.opacity = '0';
      document.body.appendChild(ta);
      ta.select();
      try {
        document.execCommand('copy');
        if (typeof OpenFangToast !== 'undefined') {
          OpenFangToast.success('Copied to clipboard');
        }
      } catch (e) {
        if (typeof OpenFangToast !== 'undefined') {
          OpenFangToast.error('Failed to copy');
        }
      }
      document.body.removeChild(ta);
      return;
    }
    navigator.clipboard.writeText(text).then(function() {
      if (typeof OpenFangToast !== 'undefined') {
        OpenFangToast.success('Copied to clipboard');
      }
    }).catch(function() {
      if (typeof OpenFangToast !== 'undefined') {
        OpenFangToast.error('Failed to copy');
      }
    });
  }

  return {
    timeAgo: timeAgo,
    formatDate: formatDate,
    formatDateTime: formatDateTime,
    statusBadge: statusBadge,
    confirmAction: confirmAction,
    truncate: truncate,
    copyToClipboard: copyToClipboard
  };
})();
