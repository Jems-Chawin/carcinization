# Dependency check: notes for Python sources

Python-specific help for `STEP.md`.

## The import name is not the package name

The tool reads import names from the code; the proposal gives the name on
PyPI. They often differ: `yaml` is `PyYAML`, `sklearn` is `scikit-learn`,
`cv2` is `opencv-python`, `PIL` is `Pillow`, `bs4` is `beautifulsoup4`. If
the library is installed, this prints the mapping:

```sh
python -c "import importlib.metadata as m; print(m.packages_distributions().get('yaml'))"
```

A wrong name is caught only if PyPI has no such project; the report prints
the project it found, so check that it is the one you meant.

## Namespace packages

`google.adk` and `google.genai` are different projects under one top-level
name. Key the proposal by `google.adk`, not `google`. `from google import
genai` is read as an import of `google.genai`.

## What "uses" shows

For `import x as y`, every attribute read through `y`. For `from x import
n`, the name `n` and what is read through it. It is what the file touches,
not what runs: a name used only in a branch never taken is still listed,
and anything reached through `getattr` or a star import is not.

## Libraries that should usually stay

NumPy, SciPy, pandas, scikit-learn, PyTorch, TensorFlow and the like do
their work in compiled code already. From Rust they are reached through
PyO3 (and the `numpy` crate for arrays without copying). Port the Python
that drives them, pass data across in bulk, and leave them where they are.
