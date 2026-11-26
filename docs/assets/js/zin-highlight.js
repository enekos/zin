// Highlights ```zin blocks, which Rouge does not know and renders as plain text.
(function () {
  var HEADINGS = /^(system|types|state|initially|always|never|sometimes|eventually|whenever|possible|terminal|when|action|fair|strongly|requires|becomes|scenario|reject|expect|include|explore|interchangeable)$/;
  var WORDS = /^(is|not|and|or|implies|of|every|some|in|satisfies|number|where|if|then|else|has|with|without|size|length|first|last|rest|plus|minus|times|divided|by|equals|different|less|than|greater|more|least|most|at|from|to)$/;
  var TYPES = /^(integer|boolean|map|record|set|list|one|a|an)$/;
  var CONSTANTS = /^(true|false|nothing)$/;
  var TOKEN = /(#.*$)|("(?:[^"\\]|\\.)*")|(\b\d+\b)|([A-Za-z_][A-Za-z0-9_]*)|([^A-Za-z0-9_#"]+)/gm;

  function escape(text) {
    return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  function span(kind, text) {
    return '<span class="zin-' + kind + '">' + escape(text) + "</span>";
  }

  function highlight(source) {
    var out = "";
    var previous = "";
    var match;
    TOKEN.lastIndex = 0;
    while ((match = TOKEN.exec(source)) !== null) {
      if (match[0] === "") { TOKEN.lastIndex++; continue; }
      if (match[1]) out += span("comment", match[1]);
      else if (match[2]) out += span("string", match[2]);
      else if (match[3]) out += span("number", match[3]);
      else if (match[4]) {
        var word = match[4];
        if (previous === "action") out += span("action", word);
        else if (HEADINGS.test(word)) out += span("keyword", word);
        else if (CONSTANTS.test(word)) out += span("constant", word);
        else if (TYPES.test(word)) out += span("type", word);
        else if (WORDS.test(word)) out += span("word", word);
        else out += escape(word);
        previous = word;
        continue;
      } else out += escape(match[5]);
      if (!/^\s*$/.test(match[0]) || /\n/.test(match[0])) previous = "";
    }
    return out;
  }

  // Kramdown leaves an unknown language as a bare <pre><code>; wrap it the way
  // Rouge wraps the languages it knows, so the theme styles it like them.
  function wrap(pre) {
    if (pre.parentNode.classList.contains("highlight")) return;
    var outer = document.createElement("div");
    outer.className = "language-zin highlighter-rouge";
    var inner = document.createElement("div");
    inner.className = "highlight";
    pre.parentNode.insertBefore(outer, pre);
    outer.appendChild(inner);
    inner.appendChild(pre);
    pre.className = "highlight";
  }

  function run() {
    var blocks = document.querySelectorAll("pre > code.language-zin, .language-zin pre code");
    for (var i = 0; i < blocks.length; i++) {
      wrap(blocks[i].parentNode);
      blocks[i].innerHTML = highlight(blocks[i].textContent);
    }
  }

  if (document.readyState === "loading") document.addEventListener("DOMContentLoaded", run);
  else run();
})();
