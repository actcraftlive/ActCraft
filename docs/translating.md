# Translating ActCraft

English is ActCraft's built-in language and the default. Every other language is one language
file in `crates/actcraft-ui/i18n/`, named by its language code: `de.tsv` (German), `fr.tsv`
(French), `pt-br.tsv` (Brazilian Portuguese). The build picks up every file there; nothing else
needs to change. The language then appears in Settings > Language under its own name.

## The file format

A language file is plain UTF-8 text in the [craft-i18n catalog format](https://github.com/benandez/craft-i18n):
one translation per line, three columns separated by a tab:

```text
<context>	<English text>	<translation>
```

- The context is usually empty, so a line starts with a tab.
- Lines starting with `#` are comments.
- The line `@id	language.name	<name>` gives the language's name in itself (`Deutsch`,
  `Français`). Every file needs it.
- `{name}` placeholders and a trailing `…` must stay as they are in the English text.
- Write `\n` for a line break, `\t` for a tab and `\\` for a backslash.

Example from `de.tsv`:

```text
@id	language.name	Deutsch
	Play	Abspielen
	Add songs…	Songs hinzufügen…
```

## Adding a language

1. Copy `de.tsv` to `<code>.tsv` and change the `language.name` row.
2. Replace the third column of every line with your translation.
3. Run `cargo test -p actcraft-ui`. The test `every_window_text_is_in_every_language_file`
   lists every window text your file doesn't have yet; `language_files_parse_cleanly` names
   lines that break the format.
4. Open a pull request.

A text missing from a file shows in English, so a running app never breaks; the test makes
sure a file that ships is complete.

## Adding window text

Every visible text goes through `tr("English text")` in `crates/actcraft-ui`. When you add one,
add its row to every language file; the test above fails until you do.

## Translating the help

The help (Help > ActCraft Help, F1) has its own folder next to the language files:

```text
crates/actcraft-ui/i18n/help/
  contents.txt        the pages in the order the contents list shows them
  en/                 the English pages, one Markdown file per page
  de/                 the German pages, same file names
  img_en/             the English pictures
  img_de/             the German pictures
```

The build embeds every page and picture; nothing else needs to change. A page or picture that a
language doesn't have yet shows in English, so you can translate page by page.

### Pages

1. Make the folder `crates/actcraft-ui/i18n/help/<code>/`, with the same code as your language
   file (`fr` for `fr.tsv`).
2. Copy a page from `en/` and translate it. Keep the file name.
3. Keep what the window and other pages rely on:
   - **Heading ids.** A heading ends in an id such as `## The mixer {#mixer}`. Translate the
     words and keep `{#mixer}` as it is: the **?** buttons and links find the part by it.
   - **Link targets.** In `[the mixer](songs-and-mixer#mixer)` translate the words in brackets,
     not the target in parentheses.
   - **Picture names.** In `![words](window.png)` translate the words (screen readers read
     them), not the file name.
   - **Names of buttons and menus.** Write them as your language file translates them, so the
     page says what the window shows.
4. Run `cargo test -p actcraft-ui help`. The tests check that every link leads to a page and
   heading there is, that every **?** button finds its part in your language, and that every
   picture a page shows exists.

The pages use plain Markdown: `#` headings, paragraphs, `**bold**` for what to press,
`*italics*`, `` `code` ``, numbered and bulleted lists, `>` quotes, tables, and pictures on a
line of their own.

### Pictures

The pictures are screenshots of ActCraft. They are not drawn or edited by hand; a command makes
them in every help language:

```sh
cargo xtask help-shots
```

It builds an example show and opens the window once per picture and language, in that language,
then saves the pictures to `img_<code>/`. Run it after you add a help folder, and again when the
window changes. It shows nothing of your computer: it starts the window without audio devices
or remote control and with settings of its own. Look at the pictures before committing them.
Their row in `ATTRIBUTION.md` covers every `img_*` folder.
