# Custom Provider Configuration

VelaClaw chat providers are protocol-only after ZS-ML-015. Custom chat
endpoints must be described as ai-protocol manifests and referenced by a
logical `provider/model` id.

## Provider Types

### Manifest-Backed Chat Endpoints

For services that implement an OpenAI-compatible, Anthropic-compatible, or
gateway-specific API, add a provider manifest to your local ai-protocol checkout
and use the logical model id from that manifest:

```toml
default_provider = "local-gateway/my-model"
default_model = "local-gateway/my-model"
```

The old `custom:https://...` and `anthropic-custom:https://...` chat provider
syntaxes now return a migration error.

## Configuration Methods

### Config File

Edit `~/.velaclaw/config.toml`:

```toml
default_provider = "local-gateway/my-model"
default_model = "local-gateway/my-model"
```

### Environment Variables

Use the credential environment variable declared by the manifest. For local
manifests that use a generic token, this is commonly:

```bash
export API_KEY="your-api-key"
velaclaw agent
```

## llama.cpp Server (Recommended Local Setup)

Use an ai-protocol manifest for `llama-server`:

- Provider/model ID example: `llamacpp/ggml-org/gpt-oss-20b-GGUF`
- Endpoint in manifest: `http://localhost:8080/v1`
- API key is optional unless `llama-server` is started with `--api-key`

Start a local server (example):

```bash
llama-server -hf ggml-org/gpt-oss-20b-GGUF --jinja -c 133000 --host 127.0.0.1 --port 8033
```

Then configure VelaClaw:

```toml
default_provider = "llamacpp/ggml-org/gpt-oss-20b-GGUF"
default_model = "llamacpp/ggml-org/gpt-oss-20b-GGUF"
default_temperature = 0.7
```

Quick validation:

```bash
velaclaw models refresh --provider llamacpp/ggml-org/gpt-oss-20b-GGUF
velaclaw agent -m "hello"
```

You do not need to export `VELACLAW_API_KEY=dummy` for this flow.

## Testing Configuration

Verify your custom manifest-backed endpoint:

```bash
# Interactive mode
velaclaw agent

# Single message test
velaclaw agent -m "test message"
```

## Troubleshooting

### Authentication Errors

- Verify API key is correct
- Check that `AI_PROTOCOL_DIR` points at the checkout containing your manifest
- Check the manifest endpoint URL format (`http://` or `https://`)
- Ensure endpoint is accessible from your network

### Model Not Found

- Confirm model name matches provider's available models
- Check provider documentation for exact model identifiers
- Ensure endpoint and model family match. Some custom gateways only expose a subset of models.
- Verify available models from the same endpoint and key you configured:

```bash
curl -sS https://your-api.com/models \
  -H "Authorization: Bearer $API_KEY"
```

- If the gateway does not implement `/models`, send a minimal chat request and inspect the provider's returned model error text.

### Connection Issues

- Test endpoint accessibility: `curl -I https://your-api.com`
- Verify firewall/proxy settings
- Check provider status page

## Examples

### Local LLM Server (Manifest-Backed Endpoint)

```toml
default_provider = "local-gateway/local-model"
default_model = "local-gateway/local-model"
```

### Corporate Proxy

```toml
default_provider = "corp-proxy/claude-sonnet"
default_model = "corp-proxy/claude-sonnet"
```

### Cloud Provider Gateway

```toml
default_provider = "cloud-gateway/gpt-4"
default_model = "cloud-gateway/gpt-4"
```
