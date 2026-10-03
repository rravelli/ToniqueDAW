# Saving projects

A project is saved as a **`.tonique`** file. It contains the arrangement,
tracks, groups, mix settings, effects, tempo and loop region.

**The audio itself is not stored in the project file.** The project only
records where each audio file is.

## New, open and save

| Menu item                   | Shortcut         |
| --------------------------- | ---------------- |
| **File → New**              | **Ctrl+N**       |
| **File → Open…**            | **Ctrl+O**       |
| **File → Open recent**      |                  |
| **File → Save**             | **Ctrl+S**       |
| **File → Save as…**         | **Ctrl+Shift+S** |

The window title shows the project's name, or *Untitled* if the project has
never been saved.

If you create a new project, open another one or close the window while you
have unsaved changes, Tonique asks whether to **Save**, **Don't save** or
**Cancel**.

**Open recent** lists the projects you opened or saved most recently, with
their folder name on the right. Projects that have been moved or deleted
are greyed out. Choose **Clear list** to empty the list.

## Keeping audio files with your project

When an audio file is inside the project's folder or one of its
subfolders, Tonique saves its location relative to the project. Files
outside that folder are saved with their full path.

**Tip:** to make a project easy to move, back up or share, copy its audio
files into the project's folder (for example into a `samples/` subfolder)
before you add them to the timeline. You can then move or copy the whole
folder as one unit.

If a file can't be found where the project expects it, Tonique also looks
for it next to the project file.

## Saving safely

Tonique first writes the project to a temporary file and only replaces the
existing file once that's done. If something goes wrong during the save,
your previous version is kept, and a message tells you what happened.

Projects saved by a newer version of Tonique can't be opened by an older
version. Update Tonique to open them.
