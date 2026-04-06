# Security Policy

## Supported Versions

| Version | Supported          |
|---------|--------------------|
| 0.1.x   | :white_check_mark: |

## Reporting a Vulnerability

If you discover a security vulnerability in dbdiff, please report it responsibly:

1. **Do not** open a public GitHub issue
2. Email the maintainers at [rekurt@users.noreply.github.com](mailto:rekurt@users.noreply.github.com)
3. Include a clear description of the vulnerability and steps to reproduce

### What to expect

- Acknowledgment within **48 hours**
- Status update within **5 business days**
- Fix or mitigation plan within **30 days** for confirmed vulnerabilities

## Security Considerations

dbdiff connects to databases using credentials provided via DSN strings. Keep in mind:

- Never commit DSN strings containing passwords to version control
- Use environment variables or secret managers for credentials
- In CI pipelines, use GitHub Secrets or equivalent
- dbdiff never stores or logs credentials
