## What changed and why

<!-- Describe the problem and resulting behavior. Link a related issue if there is one. -->

## Validation

<!-- Record the checks actually run and their results. Explain any relevant checks not run. -->

## Checklist

Mark items that do not apply as N/A. See the [contributing guide](https://github.com/JamilleJung/wireguard-tui/blob/main/CONTRIBUTING.md) for the full checks.

- [ ] Relevant formatting, lint, test and build checks are recorded above
- [ ] Dependency changes pass `cargo audit --deny warnings`
- [ ] Documentation and in-app help match changed behavior; links resolve
- [ ] UI changes include keyboard/terminal-size checks and a redacted screenshot or text example
- [ ] Helper, installer or firewall changes preserve validation, authorization, file protections, timeouts and secret redaction; relevant regression checks are recorded
- [ ] User-visible changes have a changelog entry

Do not include private keys, preshared keys, production configs, QR codes or backup archives. Report vulnerabilities through the [security policy](https://github.com/JamilleJung/wireguard-tui/blob/main/SECURITY.md).
