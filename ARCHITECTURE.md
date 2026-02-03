# Plex Rich Presence - API Reference

> Language-agnostic documentation. Focus on API contracts and data flow.

---

## Authentication

### Option A: OAuth PIN Flow (Browser)

**Step 1: Create PIN**
```
POST https://plex.tv/api/v2/pins
Headers:
  X-Plex-Product: Plex Rich Presence
  X-Plex-Client-Identifier: {unique-device-id}
  Accept: application/json

Response 201:
{
  "id": 123456789,
  "code": "ABCD",
  "authToken": null,
  "expiresAt": "2026-01-24T16:00:00Z",
  "url": "https://app.plex.tv/auth#?clientID=...&code=ABCD&..."
}
```

**Step 2: Open `url` in browser for user login**

**Step 3: Poll until `authToken` populated**
```
GET https://plex.tv/api/v2/pins/{id}
Headers:
  X-Plex-Client-Identifier: {unique-device-id}
  Accept: application/json

Response 200 (after user login):
{
  "id": 123456789,
  "authToken": "xxxxxxxxxxxxxxxxxxxxxx"
}
```

### Option B: Direct Credentials
```
POST https://plex.tv/users/sign_in.json
Headers:
  X-Plex-Product: Plex Rich Presence
  X-Plex-Client-Identifier: {unique-device-id}
Body (form):
  user[login]: username
  user[password]: password

Response 201:
{
  "user": {
    "authToken": "xxxxxxxxxxxxxxxxxxxxxx",
    "username": "username",
    "thumb": "https://plex.tv/users/.../avatar"
  }
}
```

---

## Get Account Info

```
GET https://plex.tv/api/v2/user
Headers:
  X-Plex-Token: {authToken}
  Accept: application/json

Response 200:
{
  "username": "username",
  "thumb": "https://plex.tv/users/.../avatar",
  "email": "user@example.com"
}
```

---

## List Servers

```
GET https://plex.tv/api/v2/resources?includeHttps=1&includeRelay=1
Headers:
  X-Plex-Token: {authToken}
  Accept: application/json

Response 200:
[
  {
    "name": "My Plex Server",
    "owned": true,
    "connections": [
      {
        "address": "192.168.1.100",
        "port": 32400,
        "local": true,
        "uri": "http://192.168.1.100:32400"
      }
    ]
  }
]
```

---

## Session Monitoring

### Strategy Selection
- **Owned server** → Use HTTP Polling (more reliable)
- **Shared server** → Use WebSocket (no sessions access)

### HTTP Polling (Server Owner)

```
GET http://{server}:{port}/status/sessions
Headers:
  X-Plex-Token: {authToken}
  Accept: application/json

Response 200:
{
  "MediaContainer": {
    "Metadata": [
      {
        "type": "episode",
        "title": "Episode Title",
        "index": 5,
        "parentTitle": "Season 2",
        "parentIndex": 2,
        "grandparentTitle": "Show Name",
        "duration": 2400000,
        "viewOffset": 600000,
        "thumb": "/library/metadata/123/thumb/1234567890",
        "grandparentThumb": "/library/metadata/100/thumb/1234567890",
        "Player": {
          "state": "playing"
        },
        "User": {
          "title": "username"
        }
      }
    ]
  }
}
```

**Polling Logic (pseudocode)**:
```
loop every 1000ms:
    sessions = GET /status/sessions
    user_sessions = filter(sessions, s => s.User.title == username)
    active = first_match(user_sessions, by_priority: playing > buffering > paused)
    if active != last_session:
        emit(active)
        last_session = active
```

### WebSocket (Non-Owner)

```
CONNECT ws://{server}:{port}/:/websockets/notifications?X-Plex-Token={authToken}
```

**Incoming Message Format**:
```json
{
  "NotificationContainer": {
    "type": "playing",
    "PlaySessionStateNotification": [
      {
        "key": "/library/metadata/123",
        "state": "playing",
        "viewOffset": 600000
      }
    ]
  }
}
```

**After receiving notification, fetch full metadata**:
```
GET http://{server}:{port}/library/metadata/{key}
Headers:
  X-Plex-Token: {authToken}
  Accept: application/json

Response 200:
{
  "MediaContainer": {
    "Metadata": [
      {
        "type": "episode",
        "title": "Episode Title",
        "index": 5,
        "parentTitle": "Season 2",
        "parentIndex": 2,
        "grandparentTitle": "Show Name",
        "duration": 2400000,
        "thumb": "/library/metadata/123/thumb/1234567890"
      }
    ]
  }
}
```

---

## Session Data Model

```json
{
  "mediaTitle": "Episode Title",
  "mediaIndex": 5,
  "mediaParentTitle": "Season 2",
  "mediaParentIndex": 2,
  "mediaGrandParentTitle": "Show Name",
  "playerState": "playing",
  "mediaType": "episode",
  "duration": 2400000,
  "viewOffset": 600000,
  "thumbnail": "http://server:32400/library/.../thumb?X-Plex-Token=xxx"
}
```

**playerState enum**: `playing | paused | buffering | idle`  
**mediaType enum**: `movie | episode | track | unknown | idle`

**Thumbnail URL construction**:
```
if thumb exists:
    thumbnailUrl = serverUrl + thumb.substring(1) + "?X-Plex-Token=" + token
else if grandparentThumb exists:
    thumbnailUrl = serverUrl + grandparentThumb.substring(1) + "?X-Plex-Token=" + token
else:
    thumbnailUrl = null

// Example: http://192.168.1.100:32400/library/metadata/123/thumb/1234567890?X-Plex-Token=xxx
```

---

## Discord Rich Presence

### Connection

Discord RPC uses **named pipe IPC** to local Discord client:
- Windows: `\\?\pipe\discord-ipc-0`
- macOS/Linux: `$XDG_RUNTIME_DIR/discord-ipc-0` or `$TMPDIR/discord-ipc-0`

**Application ID**: `698954724019273770`

### RPC Protocol

Uses JSON payloads over the pipe. Key operation is `SET_ACTIVITY`:

```json
{
  "cmd": "SET_ACTIVITY",
  "args": {
    "pid": 1234,
    "activity": {
      "type": 3,
      "details": "Show Name - Season 2",
      "state": "Episode Title",
      "timestamps": {
        "start": 1706097600,
        "end": 1706099400
      },
      "assets": {
        "large_image": "plex-logo",
        "large_text": "Plex",
        "small_image": "pause-circle",
        "small_text": "Paused"
      }
    }
  },
  "nonce": "unique-request-id"
}
```

**Activity type**: `3` = Watching

### Presence Mapping by Media Type

| Type | activity_type | details | state | large_image |
|------|---------------|---------|-------|-------------|
| episode | 3 (Watching) | `S{parentIndex} · E{index} — {title}` | `{grandparentTitle}` | thumbnail_url |
| movie | 3 (Watching) | `{title}` | (none) | thumbnail_url |
| track | 2 (Listening) | `{title}` | `{grandparentTitle}` | thumbnail_url |
| idle | (default) | (none) | `Idle` | (none) |
| unknown | 3 (Watching) | `{grandparentTitle} - {parentTitle}` | `{title}` | (none) |

**Activity Types**: `0` = Playing, `1` = Streaming, `2` = Listening, `3` = Watching

### Timestamps Calculation

```
if playerState == "playing":
    start = now_unix - (viewOffset / 1000)
    end = now_unix + ((duration - viewOffset) / 1000)
else:
    timestamps = null  // No countdown for paused/buffering
```

### Small Image by State

| State | small_image key |
|-------|-----------------|
| playing | (none - shows elapsed/remaining time) |
| paused | `pause-circle` |
| buffering | `sand-clock` |
| idle | `sleep-mode` |

---

## Data Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                         AUTHENTICATION                          │
├─────────────────────────────────────────────────────────────────┤
│  1. Create OAuth PIN or submit credentials                      │
│  2. Receive authToken                                           │
│  3. Store: authToken, username                                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                       SERVER SELECTION                          │
├─────────────────────────────────────────────────────────────────┤
│  1. GET /resources → list of servers                           │
│  2. User selects server                                         │
│  3. Store: serverIp, serverPort, isOwned                        │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                      SESSION MONITORING                         │
├─────────────────────────────────────────────────────────────────┤
│  if isOwned:                                                    │
│      Poll GET /status/sessions every 1s                         │
│      Filter by username                                         │
│  else:                                                          │
│      Connect WebSocket /:/websockets/notifications              │
│      On message: fetch metadata from /library/metadata/{key}    │
│                                                                 │
│  Priority: playing > buffering > paused > idle                  │
│  Emit only on change (compare all fields, 5s viewOffset drift)  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     DISCORD PRESENCE                            │
├─────────────────────────────────────────────────────────────────┤
│  1. Connect to Discord IPC pipe                                 │
│  2. Handshake with client_id                                    │
│  3. On session change: SET_ACTIVITY with mapped presence        │
│  4. On idle (optional): wait 3s then clear presence             │
└─────────────────────────────────────────────────────────────────┘
```

---

## Required HTTP Headers

All Plex API requests need:
```
X-Plex-Token: {authToken}
X-Plex-Client-Identifier: {unique-device-uuid}
X-Plex-Product: Your App Name
X-Plex-Version: 1.0.0
X-Plex-Platform: Windows/macOS/Linux
Accept: application/json
```

---

## Change Detection

Only update Discord when:
- `mediaTitle` changed
- `playerState` changed
- `mediaType` changed
- `viewOffset` drift > 5000ms from expected

---

## Idle Presence Handling

When session becomes idle:
1. Start 3-second timer
2. If new session starts within 3s, cancel timer
3. Otherwise, clear Discord presence

This prevents flickering during brief interruptions.

---

## Persistent Storage Keys

Application stores these key-value pairs locally:

| Description | Example |
|-------------|---------|
| User authentication token | `xxxxxxxxxxxxxx` |
| Plex username | `JohnDoe` |
| Selected server IP/hostname | `192.168.1.100` |
| Selected server port | `32400` |
| Whether user owns the server | `true` or `false` |
| Show presence when idle | `true` or `false` |
