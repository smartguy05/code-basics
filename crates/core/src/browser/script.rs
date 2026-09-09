//! The scripts injected into the page, and how their arguments are encoded.
//!
//! # Every script is wrapped in `try`/`catch`, because Windows swallows the
//! exception
//!
//! `wry`'s `evaluate_script_with_callback` (and Tauri's
//! `Webview::eval_with_callback`) document that an exception is **ignored on
//! Windows**, and the Phase 4 spike confirmed it: a script that did nothing but
//! `throw new Error("boom")` fired the callback with the literal string `null`
//! — indistinguishable from a script that returned `null` successfully. So an
//! unwrapped script does not fail loudly; it reports an *empty page*, an *empty
//! console*, or a *successful click that never happened*.
//!
//! Every script here therefore returns a JSON envelope —
//! `{"ok":true,"value":…}` or `{"ok":false,"error":"…"}` — and never throws.
//! A bare `null` reaching the host is then itself a fact: the wrapper is gone or
//! the page tore down mid-evaluation, which is not the same as an empty result.
//!
//! # Every argument is JSON-encoded
//!
//! These scripts are strings assembled in Rust and executed in a page. An
//! element reference or a typed string spliced in raw closes the quote and runs
//! whatever follows — `");alert(document.cookie);//` is enough. So nothing is
//! ever interpolated as bare text: [`json_string`] is the only way an argument
//! enters a script, and on top of serde's escaping it escapes `<`, `>` and `&`
//! (so a `</script>` inside a string literal cannot end an element if a script
//! is ever placed in a document rather than evaluated) plus the two Unicode
//! line terminators.
//!
//! Typed *text* gets a second, stricter check: a control character is
//! [refused][ScriptError::ControlCharacter] rather than encoded. It would encode
//! safely; the objection is that nobody asked to type it, and an escape sequence
//! sent into a page that forwards keystrokes to a terminal emulator is a
//! different kind of problem entirely.
//!
//! # Assembly is substitution, not `format!`
//!
//! Every script body is a `const` raw string with `__CB_*__` placeholders, and
//! assembly is substitution. These bodies are hundreds of characters of
//! JavaScript, which is nothing but braces; under `format!` every one of them
//! would have to be doubled, and a single missed pair is a compile error at
//! best and a silently malformed script at worst. Substitution has neither
//! failure mode, and the placeholders are ugly on purpose so they cannot be
//! mistaken for JavaScript.
//!
//! **It is [`substitute`], not a chain of [`str::replace`], and that is a
//! security property rather than a tidiness one.** `str::replace` does not
//! rescan its own replacement, but a *chain* of them rescans everything the
//! previous link emitted. `type_script` has two caller-supplied placeholders,
//! and with the reference substituted first, a reference spelling
//! `__CB_TEXT__` was emitted as the string literal `"__CB_TEXT__"` and the
//! next link then rewrote that placeholder *inside the literal it had just
//! written* — splicing the encoded text in between two quote characters, i.e.
//! into a code position. `json_string` escapes `"` and `\` but not `'`, `+`,
//! `(` or `)`, so the text could be a complete expression: `browser_type`
//! became the `browser_evaluate` that [`super::tools`] says is deliberately
//! absent. [`substitute`] walks the template once and never re-reads what it
//! has emitted, so no argument can be interpreted as another argument's
//! placeholder. Pinned by
//! `a_reference_that_spells_a_later_placeholder_cannot_smuggle_text_into_a_code_position`.
//!
//! # One non-configurable global
//!
//! The init script defines exactly one property on `window`, non-configurable
//! and non-writable, so a page cannot replace it and start feeding the host
//! fabricated console lines under this app's own message kinds. It re-entry
//! guards on its own presence, because an init script runs on every frame and
//! every navigation.

use serde_json::Value;
use thiserror::Error;

/// The single global the injected script defines on `window`.
///
/// Deliberately ugly and deliberately one: every capture buffer, the element
/// reference map and the re-entry guard hang off it, so the page's own
/// namespace gains one name rather than several.
pub const IPC_GLOBAL: &str = "__cbBrowser";

/// The largest single captured console message, in characters.
///
/// A page looping `console.log(hugeString)` would otherwise make the host
/// allocate without limit. The cut is admitted inside the text.
pub const DEFAULT_MESSAGE_LIMIT: usize = 8_000;

/// Why a script was not assembled.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ScriptError {
    /// Text destined for an input field contained a control character.
    #[error("the text to type contains a control character (U+{code:04X}) at byte {at}, so nothing was typed")]
    ControlCharacter { at: usize, code: u32 },

    /// An empty element reference. Refused, because the alternative is a script
    /// that queries for `""`, finds nothing, and reports "element not found" —
    /// blaming the page for the caller's empty argument.
    #[error("no element reference was given")]
    NoElementRef,

    /// An empty key name.
    #[error("no key was given to press")]
    NoKey,
}

/// Encode a Rust string as a JavaScript string literal, safe in an HTML context.
///
/// serde handles quotes, backslashes and control characters. The three added
/// escapes cover the case where a script is ever embedded in a document rather
/// than evaluated directly — `</script>` inside a string literal ends the
/// element regardless of JavaScript's own rules — and `\u{2028}`/`\u{2029}`
/// cover the two Unicode line terminators, which older engines treat as line
/// breaks *inside* a string literal.
pub fn json_string(value: &str) -> String {
    Value::String(value.to_owned())
        .to_string()
        .replace('<', "\\u003c")
        .replace('>', "\\u003e")
        .replace('&', "\\u0026")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029")
}

/// Substitute every `__CB_*__` placeholder in **one pass** over `template`.
///
/// Never re-reads what it has emitted, which is the whole point: see the
/// module doc. On a tie the longer placeholder wins, so `__CB_GLOBAL_NAME__` is
/// never eaten by `__CB_GLOBAL__` — the one ordering dependency a chain of
/// `replace` calls did carry, now a property of the function instead of of the
/// call order.
fn substitute(template: &str, bindings: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    loop {
        let mut best: Option<(usize, &str, &str)> = None;
        for (needle, value) in bindings {
            let Some(at) = rest.find(needle) else {
                continue;
            };
            let better = match best {
                None => true,
                Some((best_at, best_needle, _)) => {
                    at < best_at || (at == best_at && needle.len() > best_needle.len())
                }
            };
            if better {
                best = Some((at, needle, value));
            }
        }
        let Some((at, needle, value)) = best else {
            out.push_str(rest);
            return out;
        };
        out.push_str(&rest[..at]);
        out.push_str(value);
        rest = &rest[at + needle.len()..];
    }
}

/// Wrap a body so it can never throw and always answers in the envelope.
///
/// `body` must `return` a JSON string. The catch arm stringifies whatever was
/// thrown, including a thrown non-`Error` (a page can `throw 7`), and never
/// reads a property without a guard — reaching for `.message` on `null` inside
/// a catch block is how a catch block throws.
fn wrap(body: &str) -> String {
    const SHELL: &str = r#"(function(){try{__CB_BODY__}catch(e){try{return JSON.stringify({ok:false,error:String((e&&e.message)||e)})}catch(_){return "{\"ok\":false,\"error\":\"unstringifiable error\"}"}}})()"#;
    substitute(SHELL, &[("__CB_BODY__", body)])
}

/// The instrumentation installed on every page load, before the page's own
/// scripts run.
///
/// Captures `console.*`, `window.onerror`, `unhandledrejection`, the document
/// title, and network activity via a patched `fetch`/`XMLHttpRequest` plus a
/// `PerformanceObserver` over resource timings. **Every capture sits in its own
/// `try`/`catch`**: an init script that throws half-way through leaves the page
/// instrumented in an unknown, silently partial way, so each patch either
/// installs or is skipped independently and the rest still run.
pub fn init_script(max_message_chars: usize) -> String {
    substitute(
        INIT_BODY,
        &[
            ("__CB_GLOBAL_NAME__", &json_string(IPC_GLOBAL)),
            ("__CB_GLOBAL__", IPC_GLOBAL),
            ("__CB_MAX__", &max_message_chars.to_string()),
        ],
    )
}

const INIT_BODY: &str = r#"(function(){
try{
if (window.__CB_GLOBAL__) { return; }
var state = { refs: new Map(), nextRef: 1 };
try{
  Object.defineProperty(window, __CB_GLOBAL_NAME__, {
    value: state, writable: false, configurable: false, enumerable: false
  });
}catch(e){ window.__CB_GLOBAL__ = state; }

var MAX = __CB_MAX__;
function cut(s) {
  try {
    s = String(s);
    return s.length > MAX ? s.slice(0, MAX) + '… [' + s.length + ' chars, cut]' : s;
  } catch (e) { return '[unstringifiable]'; }
}
function post(msg) {
  try { window.ipc.postMessage(JSON.stringify(msg)); } catch (e) { /* the host is gone */ }
}
function render(args) {
  try {
    return Array.prototype.map.call(args, function (a) {
      if (typeof a === 'string') return a;
      try { return JSON.stringify(a); } catch (e) { return String(a); }
    }).join(' ');
  } catch (e) { return '[unrenderable arguments]'; }
}

try {
  var methods = ['debug','log','info','warn','error','trace','table','group','groupEnd','dir','assert','count'];
  for (var i = 0; i < methods.length; i++) {
    (function (name) {
      var original = console[name];
      if (typeof original !== 'function') return;
      console[name] = function () {
        try { post({ kind: 'console', method: name, text: cut(render(arguments)) }); } catch (e) {}
        try { return original.apply(console, arguments); } catch (e) {}
      };
    })(methods[i]);
  }
} catch (e) {}

try {
  window.addEventListener('error', function (event) {
    try {
      var where = event && event.filename ? ' (' + event.filename + ':' + event.lineno + ')' : '';
      post({ kind: 'pageError', source: 'onerror', text: cut(((event && (event.message || event.type)) || 'error') + where) });
    } catch (e) {}
  }, true);
  window.addEventListener('unhandledrejection', function (event) {
    try {
      var reason = event && event.reason;
      post({ kind: 'pageError', source: 'unhandledrejection', text: cut((reason && reason.message) || reason) });
    } catch (e) {}
  }, true);
} catch (e) {}

try {
  post({ kind: 'title', text: cut(document.title || '') });
  var titled = document.title;
  setInterval(function () {
    try {
      if (document.title !== titled) { titled = document.title; post({ kind: 'title', text: cut(titled || '') }); }
    } catch (e) {}
  }, 1000);
} catch (e) {}

try {
  var nativeFetch = window.fetch;
  if (typeof nativeFetch === 'function') {
    window.fetch = function (input, init) {
      var started = Date.now();
      var url = '';
      var verb = 'GET';
      try {
        url = typeof input === 'string' ? input : (input && input.url) || '';
        verb = (init && init.method) || (input && input.method) || 'GET';
      } catch (e) {}
      return nativeFetch.apply(window, arguments).then(function (response) {
        try {
          post({ kind: 'network', source: 'fetch', url: cut(url), method: String(verb).toUpperCase(),
                 status: (response && response.status) || 0, durationMs: Date.now() - started, transferSize: null });
        } catch (e) {}
        return response;
      }, function (error) {
        try {
          post({ kind: 'network', source: 'fetch', url: cut(url), method: String(verb).toUpperCase(),
                 status: null, durationMs: Date.now() - started, transferSize: null });
        } catch (e) {}
        throw error;
      });
    };
  }
} catch (e) {}

try {
  var open = XMLHttpRequest.prototype.open;
  var send = XMLHttpRequest.prototype.send;
  XMLHttpRequest.prototype.open = function (verb, url) {
    try { this.__cbVerb = String(verb || 'GET').toUpperCase(); this.__cbUrl = String(url || ''); } catch (e) {}
    return open.apply(this, arguments);
  };
  XMLHttpRequest.prototype.send = function () {
    var started = Date.now();
    var self = this;
    try {
      self.addEventListener('loadend', function () {
        try {
          post({ kind: 'network', source: 'xhr', url: cut(self.__cbUrl || ''), method: self.__cbVerb || 'GET',
                 status: self.status || null, durationMs: Date.now() - started, transferSize: null });
        } catch (e) {}
      });
    } catch (e) {}
    return send.apply(this, arguments);
  };
} catch (e) {}

try {
  new PerformanceObserver(function (list) {
    try {
      var entries = list.getEntries();
      for (var j = 0; j < entries.length; j++) {
        var entry = entries[j];
        post({ kind: 'network', source: 'resource', url: cut(entry.name || ''), method: 'GET',
               status: null, durationMs: entry.duration, transferSize: entry.transferSize || null });
      }
    } catch (e) {}
  }).observe({ type: 'resource', buffered: false });
} catch (e) {}
}catch(e){}
})();"#;

/// Read the page's rendered text.
///
/// `innerText` rather than `textContent`: `textContent` includes the contents of
/// `<script>` and `<style>` and every `display:none` element, so it is not what
/// the user is looking at. Truncation happens in Rust
/// ([`super::text::truncate_page_text`]) so the real total is reported, which a
/// page-side `slice` could not do.
pub fn page_text_script() -> String {
    wrap(PAGE_TEXT_BODY)
}

const PAGE_TEXT_BODY: &str = r#"
var t = (document.body && document.body.innerText) || '';
return JSON.stringify({ok:true,value:t,url:location.href,title:document.title||''});
"#;

/// An outline of the page's interactive and landmark elements, each with a
/// `ref`.
///
/// Refs exist so [`click_script`] and [`type_script`] address an element the
/// caller has actually *seen*, rather than a CSS selector it guessed. A guessed
/// selector on a real page is the automation failure that matters: it matches
/// something plausible and clicks the wrong control.
///
/// # `total` and `truncated` are two facts, not one
///
/// The list shrinks for two unrelated reasons: `max_elements` cut it, and a
/// match with a 0x0 box (`display:none`, a collapsed menu, `input[type=hidden]`)
/// is skipped because it cannot be clicked. `truncated` is the **cap** alone —
/// the loop stopped with matches unexamined — because that is the only thing
/// [`super::render::outline`]'s "the list is CUT" warning describes. It was
/// `found.length > out.length`, which is true on almost any real page, so the
/// warning fired over complete outlines and a genuinely capped list was
/// indistinguishable from an ordinary one. `total` stays every selector match,
/// so the renderer can state the skipped ones separately.
pub fn read_page_script(max_elements: usize) -> String {
    wrap(&substitute(
        READ_PAGE_BODY,
        &[
            ("__CB_GLOBAL__", IPC_GLOBAL),
            ("__CB_LIMIT__", &max_elements.to_string()),
        ],
    ))
}

const READ_PAGE_BODY: &str = r#"
var store = window.__CB_GLOBAL__;
if (!store) { return JSON.stringify({ok:false,error:'the page instrumentation is not installed'}); }
var selector = 'a,button,input,select,textarea,summary,[role],[contenteditable="true"],h1,h2,h3,label';
var found = document.querySelectorAll(selector);
var out = [];
var limit = __CB_LIMIT__;
var i = 0;
for (; i < found.length && out.length < limit; i++) {
  var el = found[i];
  var rect = el.getBoundingClientRect();
  if (rect.width === 0 && rect.height === 0) { continue; }
  var ref = 'e' + store.nextRef++;
  store.refs.set(ref, el);
  var name = (el.getAttribute('aria-label') || el.getAttribute('placeholder') || el.getAttribute('title') || el.value || el.innerText || '');
  name = String(name).trim();
  out.push({
    ref: ref,
    role: el.getAttribute('role') || el.tagName.toLowerCase(),
    name: name.length > 200 ? name.slice(0, 200) : name,
    type: el.getAttribute('type') || null,
    disabled: !!el.disabled
  });
}
return JSON.stringify({ok:true,value:out,total:found.length,truncated:i < found.length,url:location.href});
"#;

/// Click the element a previous `read_page` handed back as `element_ref`.
///
/// Dispatches a real click on the element rather than at a coordinate, so the
/// page's own handler runs. Reports which element it acted on, so the caller can
/// check it was the one it meant.
pub fn click_script(element_ref: &str) -> Result<String, ScriptError> {
    let encoded = encoded_ref(element_ref)?;
    Ok(wrap(&substitute(
        CLICK_BODY,
        &[("__CB_GLOBAL__", IPC_GLOBAL), ("__CB_REF__", &encoded)],
    )))
}

const CLICK_BODY: &str = r#"
var store = window.__CB_GLOBAL__;
if (!store) { return JSON.stringify({ok:false,error:'the page instrumentation is not installed'}); }
var el = store.refs.get(__CB_REF__);
if (!el) { return JSON.stringify({ok:false,error:'no element is held under that reference; read the page again'}); }
if (!el.isConnected) { return JSON.stringify({ok:false,error:'that element is no longer in the document; read the page again'}); }
el.click();
return JSON.stringify({ok:true,value:{clicked:__CB_REF__,role:el.tagName.toLowerCase()}});
"#;

/// Type `text` into the element `element_ref`.
///
/// Sets the value and fires `input` and `change`, because a framework-controlled
/// field ignores a bare value assignment. A control character in `text` is
/// refused (see the module doc).
pub fn type_script(element_ref: &str, text: &str) -> Result<String, ScriptError> {
    let encoded = encoded_ref(element_ref)?;
    if let Some((at, ch)) = text.char_indices().find(|(_, c)| c.is_control()) {
        return Err(ScriptError::ControlCharacter {
            at,
            code: ch as u32,
        });
    }
    Ok(wrap(&substitute(
        TYPE_BODY,
        &[
            ("__CB_GLOBAL__", IPC_GLOBAL),
            ("__CB_REF__", &encoded),
            ("__CB_TEXT__", &json_string(text)),
        ],
    )))
}

const TYPE_BODY: &str = r#"
var store = window.__CB_GLOBAL__;
if (!store) { return JSON.stringify({ok:false,error:'the page instrumentation is not installed'}); }
var el = store.refs.get(__CB_REF__);
if (!el) { return JSON.stringify({ok:false,error:'no element is held under that reference; read the page again'}); }
if (!el.isConnected) { return JSON.stringify({ok:false,error:'that element is no longer in the document; read the page again'}); }
el.focus();
if ('value' in el) { el.value = __CB_TEXT__; } else { el.textContent = __CB_TEXT__; }
el.dispatchEvent(new Event('input', { bubbles: true }));
el.dispatchEvent(new Event('change', { bubbles: true }));
return JSON.stringify({ok:true,value:{typedInto:__CB_REF__}});
"#;

/// Send one key to the focused element (`Enter`, `Escape`, `Tab`).
///
/// A key *name*, not text: typing goes through [`type_script`]. The name is
/// JSON-encoded like every other argument and is deliberately **not** validated
/// against a list of known keys — the page decides what it recognises, and a
/// list here would be a second, worse opinion than the DOM's own, exactly as
/// `renameLogic` refuses to validate identifiers.
pub fn press_key_script(key: &str) -> Result<String, ScriptError> {
    if key.trim().is_empty() {
        return Err(ScriptError::NoKey);
    }
    Ok(wrap(&substitute(
        PRESS_KEY_BODY,
        &[("__CB_KEY__", &json_string(key))],
    )))
}

const PRESS_KEY_BODY: &str = r#"
var el = document.activeElement || document.body;
if (!el) { return JSON.stringify({ok:false,error:'there is no focused element to send a key to'}); }
var options = { key: __CB_KEY__, bubbles: true, cancelable: true };
el.dispatchEvent(new KeyboardEvent('keydown', options));
el.dispatchEvent(new KeyboardEvent('keyup', options));
return JSON.stringify({ok:true,value:{pressed:__CB_KEY__}});
"#;

fn encoded_ref(element_ref: &str) -> Result<String, ScriptError> {
    if element_ref.trim().is_empty() {
        return Err(ScriptError::NoElementRef);
    }
    Ok(json_string(element_ref))
}

#[cfg(test)]
#[path = "script_tests.rs"]
mod tests;
