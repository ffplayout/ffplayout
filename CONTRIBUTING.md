## Contribute to ffplayout

### Report a bug

- Check issues if the bug was already reported.
- When this bug was not reported, please use the **bug report** template.
    * try to fill out every step
    * use code blocks for config, log and command line parameters
    * text from config and logging is preferred over screenshots

### Ask for help

When something is not working, you can feel free to ask your question under [discussions](https://github.com/ffplayout/ffplayout/discussions/categories/q-a). But please make some effort, so it makes it more easy to help. Please don't open discussion in a "WhatsApp style", with only one line of text. As a general rule of thumb answer this points:

- what do you want to achieve?
- what have you already tried?
- have you looked at the help documents under [/docs/](/docs)?
- what exactly is not working?
- relevant logging output
- current configuration (ffplayout.yml)

#### Sharing Screenshots

All kinds of logging and terminal outputs please share in a code block that is surrounded by **```**.

When something is wrong in the frontend you can also share as a screenshot/screen record, but please share them with English language selected.

#### Sample files

If playout works normally on your system with the [provided test clips](https://github.com/ffplayout/ffplayout/tree/master/tests/assets/media_sorted), but your files produce errors and you are sure that the problem is related to ffplayout, you can provide a test file under these conditions:
- ffmpeg can process the file normally.
- The file is not too large, a few seconds should be enough.
- The video doesn't contain any illegal content.
- You have legal permission to distribute the file.
- The content is not age restricted (no violent or sexual content).

### Feature request

You can ask for features, but it can not be guaranteed that this will find its way to the code basis. Try to think if your idea is useful for others to and describe it in a understandable way. If your idea is accepted, it can take time until it will be apply. In general stability goes over features, and when just a new version has arrived, it can take time to prove itself in production.

### Create a pull request

In general pull requests are very welcome! But please don't create features, which are to specific and helps only your use case and no one else. If your are not sure, better ask before you start.

Please also follow the code style from this project, and before you create your pull request check your code with:

```BASH
cargo fmt --all -- --check
cargo clippy --all-features --all-targets -- --deny warnings
```

For bigger changes and complied new functions a test is required.
