# Plex Discord Rich Presence

A simple and easy-to-maintain Rust application that displays your Plex media playback status as Discord Rich Presence.

![Rust](https://img.shields.io/badge/rust-%23000000.svg?style=for-the-badge&logo=rust&logoColor=white)
![Discord](https://img.shields.io/badge/Discord-%235865F2.svg?style=for-the-badge&logo=discord&logoColor=white)
![Plex](https://img.shields.io/badge/plex-%23E5A00D.svg?style=for-the-badge&logo=plex&logoColor=white)

## Features

- 🎬 Shows what you're watching on Plex in Discord
- 🎵 Supports Movies, TV Shows, and Music
- ⏯️ Displays play/pause/buffering status
- ⏱️ Shows progress and remaining time
- 🖥️ Clean and simple GUI built with egui
- 🔄 Automatic session monitoring

## Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (latest stable version)
- Discord Desktop App running on your machine
- Plex account and access to a Plex server

### OAuth Authentication

The app uses Plex's official OAuth flow for authentication:
- Your browser opens to `app.plex.tv`
- You authorize the application on Plex's website
- The app receives a secure token (your password is never stored)
- If the browser doesn't open automatically, you can copy the PIN code and URL manually

### What Gets Displayed

**For TV Shows:**
- Details: `S2 · E5 — Episode Title`
- State: `Show Name`
- Progress bar with time remaining

**For Movies:**
- Details: `Movie Title`
- Progress bar with time remaining

**For Music:**
- Details: `Track Title`
- State: `Artist/Album Name`

### Configuration

The app stores your auth token and preferences in:
- Windows: `C:\Users\<YourName>\AppData\Roaming\plex-discord-rpc\plex-discord-rpc\config.json`
- Linux: `~/.config/plex-discord-rpc/config.json`
- macOS: `~/Library/Application Support/com.plex-discord-rpc.plex-discord-rpc/config.json`

**Note**: Only the secure auth token is stored, never your password. You can manually edit this file or use the "Logout" button in the app to clear it.

## Troubleshooting

### Discord Not Showing Status
- Make sure Discord Desktop app is running
- Check that "Display current activity as a status message" is enabled in Discord settings
- Restart both the app and Discord

### Can't Connect to Plex Server
- Make sure you completed the OAuth authorization
- Ensure the server is online and accessible
- Try selecting a different server connection (local vs. remote)

### Session Not Updating
- Make sure you're playing media on the selected server
- Verify the username matches your Plex account
- Check the app shows "Monitoring Plex sessions..." in the status


## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.

## Acknowledgments

- [Plex](https://www.plex.tv/) for their media server platform
- [Discord](https://discord.com/) for Rich Presence API
- [egui](https://github.com/emilk/egui) for the excellent GUI framework

## Disclaimer

This is an unofficial third-party application and is not affiliated with, endorsed by, or connected to Plex Inc. or Discord Inc.
