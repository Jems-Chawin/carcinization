# Header art

Three headers for the top of the root README. All three are original
drawings made for this project and are under the repository's licence.
Nothing here is traced or taken from another image.

| file | what it is |
|---|---|
| `tide-line.svg` | A python that becomes a crab in five steps. The one in use. |
| `crab-pieces.svg` | A crab partly done: the left plates are solid, the right side is still a dashed outline. |
| `crab.txt` | A crab in plain characters, for a code block. |

The two SVG files use one orange (`#E0672B`) and one grey (`#8B93A1`),
chosen to be readable on a white and on a dark page, so there is one file
for both themes. They contain shapes only, with a title and a label for
screen readers: no drawn text, no font, no script, no style sheet, no link
and no embedded picture.

## Switching

The root README shows the art with one line:

    <p align="center"><img src="assets/tide-line.svg" width="560" alt="A python slithers right, grows plates and legs through a shrimp and a lobster, and ends as a crab"></p>

For the other drawing, change that line to:

    <p align="center"><img src="assets/crab-pieces.svg" width="200" alt="A crab, partly done"></p>

For the text crab, replace that line with the contents of `crab.txt` in a
code block (three backticks before and after). A code block cannot be
centred in a README on GitHub; it sits at the left.

## Looking at them without GitHub

Open an `.svg` file in a browser, or make a picture of it:

    rsvg-convert -w 560 -b '#0d1117' assets/tide-line.svg -o tide-line-dark.png
