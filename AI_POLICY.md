# AI policy

AI tools (Claude, Copilot, ChatGPT, Cursor…) are welcome in this project, including by the maintainer. What matters is what lands in the repository, not how it was typed. This policy says what we expect when you use them.

## You own what you submit

- **You are the author.** Whatever tool wrote a line, you are responsible for it. Submit only code you have read, understood and can explain in review.
- **Test it yourself.** Run the tests, run the app, try the feature on a real game when it touches mods. "The AI said it works" is not a test.
- **No unreviewed bulk changes.** Large generated diffs (reformatting, mass renames, rewritten files) are hard to review and will be closed. Keep pull requests small and focused, as you would by hand.

## Say when AI did a significant part

Add a short note in the pull request description when an AI tool wrote a significant part of the change, for example: "Parser drafted with an AI assistant, then reviewed and tested by me." It helps reviewers, and it is never held against you.

Don't add AI tools as commit co-authors (`Co-Authored-By: Claude…` and the like): the commit author is the human who answers for the change.

## Stricter areas

Code that touches players' files gets a closer review whatever wrote it: `crates/mod-engine/src/loaders/`, `deploy.rs`, `archive.rs`, `installer/`. In these areas:

- every change comes with tests on the fixtures;
- placement rules come from the source's metadata and the loader's conventions, never from a model's guess.

## AI inside the app

The app never uses AI to decide where files go or what to install. A mod description is untrusted data: it is never fed to a model to produce file operations. Any future AI feature (for example, cleaning up descriptions) must stay read-only, optional and clearly labeled.

## Issues and discussions

Write issues, reviews and comments in your own words. Generated bug reports that don't match what you saw, or invented reproduction steps, waste everyone's time and will be closed. Using a tool to translate or fix your English is fine.

## Licensing

You must have the right to submit your contribution under the project's license (GNU GPL v3 with the [UnRAR exception](LICENSE-EXCEPTION.md)). Don't submit output that reproduces code under an incompatible license.
