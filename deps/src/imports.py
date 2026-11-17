"""Lists what a Python file imports and what it uses from each import.

Run by carcinify deps as `python -I imports.py FILE...`. It reads the files
with `ast`; nothing is imported or executed, so the libraries need not be
installed. Prints one JSON object.

For each top-level `import` or `from ... import` it gives the module, where
it is, and the names the file takes from it: the names in a `from` import,
and for `import x as y` every attribute read through `y` (`y.get`,
`y.adapters.HTTPAdapter`). It also says whether the module is part of the
standard library or a file of the project, as far as that can be told
without importing.

A call to `importlib.import_module` or `__import__` with a literal name is
listed as an import too. With any other argument the name is not known
until the program runs; such calls are listed under `unread`.
"""

import ast
import json
import os
import sys
import sysconfig


def stdlib(top):
    names = getattr(sys, "stdlib_module_names", None)
    if names is not None:
        return top in names
    if top in sys.builtin_module_names:
        return True
    lib = sysconfig.get_paths()["stdlib"]
    return os.path.exists(os.path.join(lib, top + ".py")) or os.path.isdir(os.path.join(lib, top))


def local(top, start):
    """True if `top` is a module or package beside the file or in a
    directory above it, up to the repository root."""
    d = os.path.dirname(os.path.abspath(start))
    while True:
        if os.path.exists(os.path.join(d, top + ".py")) or os.path.exists(os.path.join(d, top, "__init__.py")):
            return True
        if os.path.exists(os.path.join(d, ".git")) or os.path.dirname(d) == d:
            return False
        d = os.path.dirname(d)


def scan(path):
    with open(path, encoding="utf-8") as f:
        tree = ast.parse(f.read(), path)
    found = {}  # module -> {"lines": set, "names": set}
    aliases = {}  # local name -> module it stands for
    from_names = {}  # local name -> (module, name imported from it)

    def entry(module):
        return found.setdefault(module, {"lines": set(), "names": set()})

    for node in ast.walk(tree):
        if isinstance(node, ast.Import):
            for a in node.names:
                entry(a.name)["lines"].add(node.lineno)
                # `import a.b` binds `a`; `import a.b as c` binds `c` to a.b
                aliases[a.asname or a.name.split(".")[0]] = a.name if a.asname else a.name.split(".")[0]
        elif isinstance(node, ast.ImportFrom):
            if node.level:  # relative: the project's own code
                continue
            e = entry(node.module)
            e["lines"].add(node.lineno)
            e["names"].update(a.name for a in node.names)
            for a in node.names:
                # `from m import n` binds `n`; what is read through it later
                # (`n.x.y`) is recorded as `n.x.y` of `m`
                from_names[a.asname or a.name] = (node.module, a.name)
    # Imports made by a call: `importlib.import_module("yaml")`,
    # `__import__("zeep")`. A literal name is an import like any other. Any
    # other argument cannot be read here, and is reported as such.
    unread = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        f = node.func
        called = None
        if isinstance(f, ast.Name) and f.id == "__import__":
            called = "__import__"
        elif isinstance(f, ast.Name) and from_names.get(f.id) == ("importlib", "import_module"):
            called = "import_module"
        elif isinstance(f, ast.Attribute) and f.attr == "import_module" and isinstance(f.value, ast.Name) and aliases.get(f.value.id) == "importlib":
            called = "importlib.import_module"
        elif isinstance(f, ast.Attribute) and f.attr == "__import__" and isinstance(f.value, ast.Name) and f.value.id in ("builtins", "__builtins__"):
            called = "builtins.__import__"
        if called is None:
            continue
        arg = node.args[0] if node.args else None
        name = getattr(arg, "value", None) if isinstance(arg, ast.Constant) else None
        if isinstance(name, str) and name and not name.startswith("."):
            e = entry(name)
            e["lines"].add(node.lineno)
            e["run_time"] = True
        elif not (isinstance(name, str) and name.startswith(".")):
            try:
                text = ast.unparse(node)
            except Exception:  # Python 3.8
                text = called + "(...)"
            unread.append({"line": node.lineno, "call": text[:120]})
    for node in ast.walk(tree):
        if isinstance(node, ast.Attribute):
            chain = []
            cur = node
            while isinstance(cur, ast.Attribute):
                chain.append(cur.attr)
                cur = cur.value
            if isinstance(cur, ast.Name) and cur.id in from_names:
                module, name = from_names[cur.id]
                entry(module)["names"].add(".".join([name] + list(reversed(chain))))
            elif isinstance(cur, ast.Name) and cur.id in aliases:
                module = aliases[cur.id]
                # attribute to the longest imported module the chain runs through
                parts = list(reversed(chain))
                while parts and (module + "." + parts[0]) in found:
                    module = module + "." + parts.pop(0)
                if parts:
                    entry(module)["names"].add(".".join(parts))
    out = []
    for module in sorted(found):
        top = module.split(".")[0]
        kind = "standard_library" if stdlib(top) else "local" if local(top, path) else "third_party"
        names = sorted(found[module]["names"])
        # keep the longest chains only: `a.b` makes `a` redundant
        names = [n for n in names if not any(o.startswith(n + ".") for o in names)]
        out.append({"module": module, "kind": kind, "lines": sorted(found[module]["lines"]), "uses": names,
                    "imported_at_run_time": bool(found[module].get("run_time"))})
    return out, sorted(unread, key=lambda u: u["line"])


def distributions(tops):
    """For each top-level module name: the installed projects that provide
    it, as this interpreter knows them. Empty when it is not installed."""
    try:
        import importlib.metadata as md
    except ImportError:  # Python 3.7
        return {}
    known = {}
    try:
        known = {k: list(v) for k, v in md.packages_distributions().items()}
    except AttributeError:  # before Python 3.10: read each project's file list
        for dist in md.distributions():
            name = dist.metadata["Name"]
            names = set((dist.read_text("top_level.txt") or "").split())
            for f in dist.files or []:
                parts = str(f).replace("\\", "/").split("/")
                if len(parts) == 1 and parts[0].endswith(".py"):
                    names.add(parts[0][:-3])
                elif len(parts) > 1 and "." not in parts[0]:
                    names.add(parts[0])
            for top in names:
                known.setdefault(top, [])
                if name and name not in known[top]:
                    known[top].append(name)
    except Exception:
        return {}
    return {top: sorted(known.get(top, [])) for top in tops if known.get(top)}


if __name__ == "__main__":
    scanned = {p: scan(p) for p in sys.argv[1:]}
    print(json.dumps({"python": "%d.%d.%d" % sys.version_info[:3],
                      "files": {p: s[0] for p, s in scanned.items()},
                      "unread": {p: s[1] for p, s in scanned.items() if s[1]},
                      "distributions": distributions(sorted({i["module"].split(".")[0] for s in scanned.values() for i in s[0] if i["kind"] == "third_party"}))}))
