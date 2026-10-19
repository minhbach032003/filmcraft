# FilmCraft AI Director

AI Director enables automated end-to-end screenwriting and editing inside FilmCraft: converting raw screenplays into structured shot plans, generating visual references via Imagen 3, video previews via Veo 2, synthesizing character dialogue, and constructing full multi-track timeline sequences.

## 1. Quick Start

### 1.1 API Key Configuration

To use Gemini, Imagen 3, or Veo 2, configure your Gemini API key through one of the following methods (in order of precedence):

1. **Environment Variable**:
   ```bash
   export FILMCRAFT_GEMINI_API_KEY="AIzaSy..."
   ```
2. **In-App AI Director Panel**:
   Open **Window ▸ AI Director**, enter your key, and click **Save Key**.
   The key is kept securely in process memory and never written to project files, undo journals, or application logs.
3. **Headless Engine Command / MCP**:
   ```json
   { "command": "ai.setKey", "params": { "key": "AIzaSy..." } }
   ```

### 1.2 Offline Usage (Zero Cost, No Network)

FilmCraft's AI Director works 100% offline without an API key:
- **Screenplay Parsing**: Built-in parser for standard Fountain syntax (`INT./EXT.`, dialogue, character cues).
- **Voice Synthesis**: Built-in formant voices (`basic-female`, `basic-male`) generate mono speech without any model download.
- **Timeline Assembly**: `ai.buildSequence` constructs video tracks, scene markers, and subtitle caption tracks with exact `Tick` timing.

## 2. Command Reference

All operations are standard, undoable FilmCraft commands:

| Command | Type | Parameters | Description |
|---|---|---|---|
| `ai.setKey` | Query | `{"key": str}` | Sets active Gemini API key in process memory. |
| `ai.clearKey` | Query | `{}` | Clears active API key from memory. |
| `ai.status` | Query | `{}` | Returns whether an API key is configured and available models. |
| `ai.characterAdd` | Edit | `{"id": str, "name": str, "description": str, "aliases"?: [str], "voice_id"?: str}` | Adds/updates a character in the project Character Library. |
| `ai.characterRemove` | Edit | `{"id": str}` | Removes a character from the project. |
| `ai.characterList` | Query | `{}` | Lists all registered characters in the current project. |
| `ai.planScript` | Query | `{"script": str, "title"?: str, "mode"?: ("fountain" \| "ai"), "dry_run"?: bool}` | Parses text script into a validated `StoryPlan`. |
| `ai.buildSequence` | Edit | `{"plan": StoryPlan, "name"?: str, "fps"?: f64, "width"?: u32, "height"?: u32}` | Synthesizes a timeline sequence with tracks, clips, markers, and dialogue captions. Single-step undo. |

## 3. Automation & MCP Driving

Agents can inspect project state and automate assembly via standard MCP tools:

```bash
# 1. Check status
mcp call command_run '{"id": "ai.status", "params": {}}'

# 2. Add characters
mcp call command_run '{"id": "ai.characterAdd", "params": {"id": "c_maya", "name": "Maya", "description": "Pilot in orange suit"}}'

# 3. Ingest script
mcp call command_run '{"id": "ai.planScript", "params": {"script": "INT. COCKPIT\n\nMAYA\nInitiating descent.\n", "title": "Descent"}}'

# 4. Build sequence
mcp call command_run '{"id": "ai.buildSequence", "params": {"plan": <StoryPlan>, "name": "Descent Sequence"}}'
```
