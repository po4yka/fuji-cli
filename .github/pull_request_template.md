# Pull Request

## Summary

Describe the observable change and why it belongs in the selected
source-of-truth layer.

## Validation

List the exact checks run and their outcomes. Separate local, fixture,
generated-code, hosted-CI, and physical-camera evidence.

## Camera Safety and Privacy

State whether the change can read or mutate camera state. For a physical-device
run, record the exact model, firmware, USB mode, command, and semantic outcome.
Do not attach camera serial numbers, backup artifacts, private RAF/JPEG files,
custom-setting names, or unreviewed `-vvv` output.

## Checklist

- [ ] The change is focused and updates every affected call site or contract.
- [ ] Behavior changes have relevant regression and failure-path tests.
- [ ] Generated output was produced only through the build and was not committed.
- [ ] User documentation and the support table claim only verified behavior.
- [ ] Public text and attachments were privacy-reviewed.
- [ ] I followed the [contributor guide](../docs/contributors/README.md) and
      [Code of Conduct](../CODE_OF_CONDUCT.md).
