# Friend Reader Server

HTTP server that parses EPUB files and serves them to connected clients with real-time position tracking.

## Build

```bash
cargo build --release
```

## Usage

Basic usage (no password):
```bash
./target/release/server path/to/book.epub
```

With password protection:
```bash
./target/release/server path/to/book.epub --password "your_password_here"
```

The server listens on `0.0.0.0:15470` by default.

## API Endpoints

### GET /health
Health check endpoint. Returns server status and whether password is required.

Response:
```json
{
  "status": "ok",
  "requires_password": false
}
```

### GET /document
Returns the full document structure.

Query params:
- `password_hash` (optional): SHA256 hash of password if server has password protection

Response:
```json
{
  "metadata": {
    "title": "Book Title",
    "language": "en",
    "author": "Author Name"
  },
  "elements": [
    { "type": "text", "content": "Paragraph text..." },
    { "type": "heading", "content": "Chapter 1", "level": 1 },
    { "type": "image", "id": "img_001", "url": "/images/img_001" }
  ]
}
```

### GET /images/{id}
Returns an image by ID.

Query params:
- `password_hash` (optional): SHA256 hash of password if server has password protection

### GET /positions
Returns all connected users and their current reading positions.

Query params:
- `password_hash` (optional): SHA256 hash of password if server has password protection

Response:
```json
{
  "users": {
    "Alice:#FF0000": {
      "name": "Alice",
      "color": "#FF0000",
      "position": {
        "start_element": 10,
        "start_percent": 0.5,
        "end_element": 15,
        "end_percent": 0.8
      }
    }
  }
}
```

### POST /update_position
Updates the current user's reading position.

Request body:
```json
{
  "name": "Alice",
  "color": "#FF0000",
  "position": {
    "start_element": 10,
    "start_percent": 0.5,
    "end_element": 15,
    "end_percent": 0.8
  },
  "password_hash": null
}
```

## Features

- EPUB parsing with text and image support
- Real-time position tracking for multiple users
- Automatic heartbeat system (removes users after 10 seconds of inactivity)
- Optional password protection with SHA256 hashing
- CORS enabled for easy client development
- Support for English, Japanese, and Chinese text

## Testing

```bash
curl http://localhost:15470/health

curl http://localhost:15470/document | jq '.metadata'

curl -X POST http://localhost:15470/update_position \
  -H "Content-Type: application/json" \
  -d '{"name":"Alice","color":"#FF0000","position":{"start_element":0,"start_percent":0.0,"end_element":5,"end_percent":0.5},"password_hash":null}'

curl http://localhost:15470/positions | jq
```

