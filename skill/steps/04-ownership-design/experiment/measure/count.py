"""Counts, in a port's source, the constructs the ownership step is about.

Two scopes:
- `hot`: functions reachable from `tick` (the method named `tick`, then any
  function of the crate whose name is called from a reachable body);
- `crate`: every `.rs` file under `src/`.

It is a text count with brace matching, not a parser: a name called through
a trait or a closure is matched by name only, and comments are stripped
first. Good enough to compare runs of the same task; read the code before
drawing anything finer from it.

    python3 count.py path/to/relay/src
"""

import json
import os
import re
import sys

PATTERNS = {
    "clone": r"\.clone\(\)|\.cloned\(\)",
    "collect": r"\.collect\b",
    "to_vec_owned_string": r"\.to_vec\(\)|\.to_owned\(\)|\.to_string\(\)",
    "mem_take_replace": r"mem::take|mem::replace|mem::swap|\.take\(\)",
    "rc_refcell_arc_mutex": r"\bRc<|\bRefCell<|\bArc<|\bMutex<|\bRwLock<|\bCell<|\.borrow_mut\(\)|\.borrow\(\)|\.lock\(\)",
    "vec_new_in_body": r"Vec::new\(\)|Vec::with_capacity|vec!\[|VecDeque::new\(\)|String::new\(\)|format!",
}


def strip_comments(text):
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//[^\n]*", "", text)


def without_tests(text):
    """Drops test code, which does not run on the hot path: a file that is
    all test (`#![cfg(test)]`), and every item that follows a `#[cfg(test)]`
    attribute, up to its closing brace or semicolon."""
    if "#![cfg(test)]" in text:
        return ""
    while True:
        m = re.search(r"#\[cfg\(test\)\]", text)
        if not m:
            return text
        brace, semi = text.find("{", m.end()), text.find(";", m.end())
        if brace < 0 or (0 <= semi < brace):
            end = semi if semi >= 0 else len(text) - 1
        else:
            depth, end = 0, brace
            while end < len(text):
                if text[end] == "{":
                    depth += 1
                elif text[end] == "}":
                    depth -= 1
                    if depth == 0:
                        break
                end += 1
        text = text[: m.start()] + text[end + 1 :]


def functions(text):
    """name -> list of bodies (text between the function's braces)."""
    out = {}
    for m in re.finditer(r"\bfn\s+([A-Za-z_][A-Za-z0-9_]*)", text):
        i = text.find("{", m.end())
        semi = text.find(";", m.end())
        if i < 0 or (0 <= semi < i):
            continue
        depth, j = 0, i
        while j < len(text):
            if text[j] == "{":
                depth += 1
            elif text[j] == "}":
                depth -= 1
                if depth == 0:
                    break
            j += 1
        out.setdefault(m.group(1), []).append(text[i : j + 1])
    return out


def main(src):
    text = ""
    for root, _, files in os.walk(src):
        for name in sorted(files):
            # A module file named for tests (alloc_test.rs) is test code too.
            if name.endswith(".rs") and "test" not in name:
                with open(os.path.join(root, name), encoding="utf-8") as f:
                    text += without_tests(strip_comments(f.read())) + "\n"
    fns = functions(text)
    reachable, todo = set(), ["tick"]
    while todo:
        name = todo.pop()
        if name in reachable or name not in fns:
            continue
        reachable.add(name)
        for body in fns[name]:
            for called in set(re.findall(r"\b([A-Za-z_][A-Za-z0-9_]*)\s*(?:::<[^>]*>)?\(", body)):
                if called in fns and called not in reachable:
                    todo.append(called)
    hot = "\n".join(body for name in sorted(reachable) for body in fns[name])
    count = lambda scope: {k: len(re.findall(p, scope)) for k, p in PATTERNS.items()}
    print(json.dumps({"hot_functions": sorted(reachable), "hot": count(hot), "crate": count(text), "lines": text.count("\n")}))


if __name__ == "__main__":
    main(sys.argv[1])
