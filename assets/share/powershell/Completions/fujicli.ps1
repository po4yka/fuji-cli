
using namespace System.Management.Automation
using namespace System.Management.Automation.Language

Register-ArgumentCompleter -Native -CommandName 'fujicli' -ScriptBlock {
    param($wordToComplete, $commandAst, $cursorPosition)

    $commandElements = $commandAst.CommandElements
    $command = @(
        'fujicli'
        for ($i = 1; $i -lt $commandElements.Count; $i++) {
            $element = $commandElements[$i]
            if ($element -isnot [StringConstantExpressionAst] -or
                $element.StringConstantType -ne [StringConstantType]::BareWord -or
                $element.Value.StartsWith('-') -or
                $element.Value -eq $wordToComplete) {
                break
        }
        $element.Value
    }) -join ';'

    $completions = @(switch ($command) {
        'fujicli' {
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('-V', '-V ', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('--version', '--version', [CompletionResultType]::ParameterName, 'Print version')
            [CompletionResult]::new('device', 'device', [CompletionResultType]::ParameterValue, 'Manage devices')
            [CompletionResult]::new('simulation', 'simulation', [CompletionResultType]::ParameterValue, 'Manage film simulations')
            [CompletionResult]::new('backup', 'backup', [CompletionResultType]::ParameterValue, 'Manage backups')
            [CompletionResult]::new('image', 'image', [CompletionResultType]::ParameterValue, 'Manage and render images')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;device' {
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List cameras')
            [CompletionResult]::new('info', 'info', [CompletionResultType]::ParameterValue, 'Get camera info')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;device;list' {
            [CompletionResult]::new('-j', '-j', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;device;info' {
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--emulate', '--emulate', [CompletionResultType]::ParameterName, 'Treat device as a different model using <VENDOR_ID>:<PRODUCT_ID>')
            [CompletionResult]::new('-j', '-j', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;device;help' {
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List cameras')
            [CompletionResult]::new('info', 'info', [CompletionResultType]::ParameterValue, 'Get camera info')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;device;help;list' {
            break
        }
        'fujicli;device;help;info' {
            break
        }
        'fujicli;device;help;help' {
            break
        }
        'fujicli;simulation' {
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List simulations')
            [CompletionResult]::new('get', 'get', [CompletionResultType]::ParameterValue, 'Get simulation')
            [CompletionResult]::new('set', 'set', [CompletionResultType]::ParameterValue, 'Set simulation parameters')
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Export simulation')
            [CompletionResult]::new('import', 'import', [CompletionResultType]::ParameterValue, 'Import simulation')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;simulation;list' {
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('-j', '-j', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;simulation;get' {
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('-j', '-j', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;simulation;set' {
            [CompletionResult]::new('--clarity', '--clarity', [CompletionResultType]::ParameterName, 'Clarity')
            [CompletionResult]::new('--color', '--color', [CompletionResultType]::ParameterName, 'Color')
            [CompletionResult]::new('--color-chrome-effect', '--color-chrome-effect', [CompletionResultType]::ParameterName, 'Color Chrome Effect')
            [CompletionResult]::new('--color-chrome-fx-blue', '--color-chrome-fx-blue', [CompletionResultType]::ParameterName, 'Color Chrome FX Blue')
            [CompletionResult]::new('--color-space', '--color-space', [CompletionResultType]::ParameterName, 'Color Space')
            [CompletionResult]::new('--custom-setting-name', '--custom-setting-name', [CompletionResultType]::ParameterName, 'Custom Setting Name')
            [CompletionResult]::new('--dynamic-range', '--dynamic-range', [CompletionResultType]::ParameterName, 'Dynamic Range')
            [CompletionResult]::new('--dynamic-range-priority', '--dynamic-range-priority', [CompletionResultType]::ParameterName, 'Dynamic Range Priority')
            [CompletionResult]::new('--film-simulation', '--film-simulation', [CompletionResultType]::ParameterName, 'Film Simulation')
            [CompletionResult]::new('--grain-effect', '--grain-effect', [CompletionResultType]::ParameterName, 'Grain Effect')
            [CompletionResult]::new('--highlight-tone', '--highlight-tone', [CompletionResultType]::ParameterName, 'Highlight Tone')
            [CompletionResult]::new('--image-quality', '--image-quality', [CompletionResultType]::ParameterName, 'Image Quality')
            [CompletionResult]::new('--image-size', '--image-size', [CompletionResultType]::ParameterName, 'Image Size')
            [CompletionResult]::new('--lens-modulation-optimizer', '--lens-modulation-optimizer', [CompletionResultType]::ParameterName, 'Lens Modulation Optimizer')
            [CompletionResult]::new('--monochromatic-color-temperature', '--monochromatic-color-temperature', [CompletionResultType]::ParameterName, 'Monochromatic Color Temperature')
            [CompletionResult]::new('--monochromatic-color-tint', '--monochromatic-color-tint', [CompletionResultType]::ParameterName, 'Monochromatic Color Tint')
            [CompletionResult]::new('--noise-reduction', '--noise-reduction', [CompletionResultType]::ParameterName, 'High ISO Noise Reduction')
            [CompletionResult]::new('--shadow-tone', '--shadow-tone', [CompletionResultType]::ParameterName, 'Shadow Tone')
            [CompletionResult]::new('--sharpness', '--sharpness', [CompletionResultType]::ParameterName, 'Sharpness')
            [CompletionResult]::new('--smooth-skin-effect', '--smooth-skin-effect', [CompletionResultType]::ParameterName, 'Smooth Skin Effect')
            [CompletionResult]::new('--white-balance', '--white-balance', [CompletionResultType]::ParameterName, 'White Balance')
            [CompletionResult]::new('--white-balance-shift-blue', '--white-balance-shift-blue', [CompletionResultType]::ParameterName, 'White Balance Shift Blue')
            [CompletionResult]::new('--white-balance-shift-red', '--white-balance-shift-red', [CompletionResultType]::ParameterName, 'White Balance Shift Red')
            [CompletionResult]::new('--white-balance-temperature', '--white-balance-temperature', [CompletionResultType]::ParameterName, 'White Balance Temperature')
            [CompletionResult]::new('--target-serial-sha256', '--target-serial-sha256', [CompletionResultType]::ParameterName, 'SHA-256 fingerprint of the exact physical camera serial number')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            break
        }
        'fujicli;simulation;export' {
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--force', '--force', [CompletionResultType]::ParameterName, 'Replace an existing regular output file')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;simulation;import' {
            [CompletionResult]::new('--target-serial-sha256', '--target-serial-sha256', [CompletionResultType]::ParameterName, 'SHA-256 fingerprint of the exact physical camera serial number')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;simulation;help' {
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List simulations')
            [CompletionResult]::new('get', 'get', [CompletionResultType]::ParameterValue, 'Get simulation')
            [CompletionResult]::new('set', 'set', [CompletionResultType]::ParameterValue, 'Set simulation parameters')
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Export simulation')
            [CompletionResult]::new('import', 'import', [CompletionResultType]::ParameterValue, 'Import simulation')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;simulation;help;list' {
            break
        }
        'fujicli;simulation;help;get' {
            break
        }
        'fujicli;simulation;help;set' {
            break
        }
        'fujicli;simulation;help;export' {
            break
        }
        'fujicli;simulation;help;import' {
            break
        }
        'fujicli;simulation;help;help' {
            break
        }
        'fujicli;backup' {
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Export backup')
            [CompletionResult]::new('inspect', 'inspect', [CompletionResultType]::ParameterValue, 'Inspect and validate a backup artifact without connecting a camera')
            [CompletionResult]::new('import', 'import', [CompletionResultType]::ParameterValue, 'Import backup')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;backup;export' {
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--force', '--force', [CompletionResultType]::ParameterName, 'Replace an existing regular output file')
            [CompletionResult]::new('-j', '-j', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;backup;inspect' {
            [CompletionResult]::new('-j', '-j', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Format output using JSON')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;backup;import' {
            [CompletionResult]::new('--recovery-backup', '--recovery-backup', [CompletionResultType]::ParameterName, 'New file that receives the target''s current settings before restore')
            [CompletionResult]::new('--expect-sha256', '--expect-sha256', [CompletionResultType]::ParameterName, 'Expected SHA-256 of the complete input artifact')
            [CompletionResult]::new('--target-serial-sha256', '--target-serial-sha256', [CompletionResultType]::ParameterName, 'Expected SHA-256 fingerprint of a different target camera serial')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--yes', '--yes', [CompletionResultType]::ParameterName, 'Confirm sending the validated backup artifact to the selected camera')
            [CompletionResult]::new('--dry-run', '--dry-run', [CompletionResultType]::ParameterName, 'Validate compatibility without exporting recovery state or restoring')
            [CompletionResult]::new('--allow-stdin', '--allow-stdin', [CompletionResultType]::ParameterName, 'Permit destructive import from stdin (also requires --expect-sha256)')
            [CompletionResult]::new('-j', '-j', [CompletionResultType]::ParameterName, 'Format dry-run output using JSON')
            [CompletionResult]::new('--json', '--json', [CompletionResultType]::ParameterName, 'Format dry-run output using JSON')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;backup;help' {
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Export backup')
            [CompletionResult]::new('inspect', 'inspect', [CompletionResultType]::ParameterValue, 'Inspect and validate a backup artifact without connecting a camera')
            [CompletionResult]::new('import', 'import', [CompletionResultType]::ParameterValue, 'Import backup')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;backup;help;export' {
            break
        }
        'fujicli;backup;help;inspect' {
            break
        }
        'fujicli;backup;help;import' {
            break
        }
        'fujicli;backup;help;help' {
            break
        }
        'fujicli;image' {
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('render', 'render', [CompletionResultType]::ParameterValue, 'Render image')
            [CompletionResult]::new('recover', 'recover', [CompletionResultType]::ParameterValue, 'Recover a retained rendered JPEG by its camera object handle')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;image;render' {
            [CompletionResult]::new('--target-serial-sha256', '--target-serial-sha256', [CompletionResultType]::ParameterName, 'SHA-256 fingerprint of the exact physical camera serial number')
            [CompletionResult]::new('--simulation-file', '--simulation-file', [CompletionResultType]::ParameterName, 'Path to exported simulation file')
            [CompletionResult]::new('--clarity', '--clarity', [CompletionResultType]::ParameterName, 'Clarity')
            [CompletionResult]::new('--color', '--color', [CompletionResultType]::ParameterName, 'Color')
            [CompletionResult]::new('--color-chrome-effect', '--color-chrome-effect', [CompletionResultType]::ParameterName, 'Color Chrome Effect')
            [CompletionResult]::new('--color-chrome-fx-blue', '--color-chrome-fx-blue', [CompletionResultType]::ParameterName, 'Color Chrome FX Blue')
            [CompletionResult]::new('--color-space', '--color-space', [CompletionResultType]::ParameterName, 'Color Space')
            [CompletionResult]::new('--dynamic-range', '--dynamic-range', [CompletionResultType]::ParameterName, 'Dynamic Range')
            [CompletionResult]::new('--dynamic-range-priority', '--dynamic-range-priority', [CompletionResultType]::ParameterName, 'Dynamic Range Priority')
            [CompletionResult]::new('--exposure-offset', '--exposure-offset', [CompletionResultType]::ParameterName, 'Exposure Offset')
            [CompletionResult]::new('--file-type', '--file-type', [CompletionResultType]::ParameterName, 'File Type')
            [CompletionResult]::new('--film-simulation', '--film-simulation', [CompletionResultType]::ParameterName, 'Film Simulation')
            [CompletionResult]::new('--grain-effect', '--grain-effect', [CompletionResultType]::ParameterName, 'Grain Effect')
            [CompletionResult]::new('--highlight-tone', '--highlight-tone', [CompletionResultType]::ParameterName, 'Highlight Tone')
            [CompletionResult]::new('--image-quality', '--image-quality', [CompletionResultType]::ParameterName, 'Image Quality')
            [CompletionResult]::new('--image-size', '--image-size', [CompletionResultType]::ParameterName, 'Image Size')
            [CompletionResult]::new('--lens-modulation-optimizer', '--lens-modulation-optimizer', [CompletionResultType]::ParameterName, 'Lens Modulation Optimizer')
            [CompletionResult]::new('--monochromatic-color-temperature', '--monochromatic-color-temperature', [CompletionResultType]::ParameterName, 'Monochromatic Color Temperature')
            [CompletionResult]::new('--monochromatic-color-tint', '--monochromatic-color-tint', [CompletionResultType]::ParameterName, 'Monochromatic Color Tint')
            [CompletionResult]::new('--noise-reduction', '--noise-reduction', [CompletionResultType]::ParameterName, 'High ISO Noise Reduction')
            [CompletionResult]::new('--shadow-tone', '--shadow-tone', [CompletionResultType]::ParameterName, 'Shadow Tone')
            [CompletionResult]::new('--sharpness', '--sharpness', [CompletionResultType]::ParameterName, 'Sharpness')
            [CompletionResult]::new('--smooth-skin-effect', '--smooth-skin-effect', [CompletionResultType]::ParameterName, 'Smooth Skin Effect')
            [CompletionResult]::new('--teleconverter', '--teleconverter', [CompletionResultType]::ParameterName, 'Teleconverter')
            [CompletionResult]::new('--white-balance', '--white-balance', [CompletionResultType]::ParameterName, 'White Balance')
            [CompletionResult]::new('--white-balance-shift-blue', '--white-balance-shift-blue', [CompletionResultType]::ParameterName, 'White Balance Shift Blue')
            [CompletionResult]::new('--white-balance-shift-red', '--white-balance-shift-red', [CompletionResultType]::ParameterName, 'White Balance Shift Red')
            [CompletionResult]::new('--white-balance-temperature', '--white-balance-temperature', [CompletionResultType]::ParameterName, 'White Balance Temperature')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--draft', '--draft', [CompletionResultType]::ParameterName, 'Render a lower-quality (faster) preview')
            [CompletionResult]::new('--force', '--force', [CompletionResultType]::ParameterName, 'Replace an existing regular output file')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help (see more with ''--help'')')
            break
        }
        'fujicli;image;recover' {
            [CompletionResult]::new('--target-serial-sha256', '--target-serial-sha256', [CompletionResultType]::ParameterName, 'SHA-256 fingerprint of the exact physical camera serial number')
            [CompletionResult]::new('-d', '-d', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--device', '--device', [CompletionResultType]::ParameterName, 'Manually specify target device using USB <BUS>.<ADDRESS>')
            [CompletionResult]::new('--force', '--force', [CompletionResultType]::ParameterName, 'Replace an existing regular output file')
            [CompletionResult]::new('--delete-after-save', '--delete-after-save', [CompletionResultType]::ParameterName, 'Delete the camera object after a verified local file save')
            [CompletionResult]::new('-v', '-v', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('--verbose', '--verbose', [CompletionResultType]::ParameterName, 'Log extra debugging information (multiple instances increase verbosity)')
            [CompletionResult]::new('-h', '-h', [CompletionResultType]::ParameterName, 'Print help')
            [CompletionResult]::new('--help', '--help', [CompletionResultType]::ParameterName, 'Print help')
            break
        }
        'fujicli;image;help' {
            [CompletionResult]::new('render', 'render', [CompletionResultType]::ParameterValue, 'Render image')
            [CompletionResult]::new('recover', 'recover', [CompletionResultType]::ParameterValue, 'Recover a retained rendered JPEG by its camera object handle')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;image;help;render' {
            break
        }
        'fujicli;image;help;recover' {
            break
        }
        'fujicli;image;help;help' {
            break
        }
        'fujicli;help' {
            [CompletionResult]::new('device', 'device', [CompletionResultType]::ParameterValue, 'Manage devices')
            [CompletionResult]::new('simulation', 'simulation', [CompletionResultType]::ParameterValue, 'Manage film simulations')
            [CompletionResult]::new('backup', 'backup', [CompletionResultType]::ParameterValue, 'Manage backups')
            [CompletionResult]::new('image', 'image', [CompletionResultType]::ParameterValue, 'Manage and render images')
            [CompletionResult]::new('help', 'help', [CompletionResultType]::ParameterValue, 'Print this message or the help of the given subcommand(s)')
            break
        }
        'fujicli;help;device' {
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List cameras')
            [CompletionResult]::new('info', 'info', [CompletionResultType]::ParameterValue, 'Get camera info')
            break
        }
        'fujicli;help;device;list' {
            break
        }
        'fujicli;help;device;info' {
            break
        }
        'fujicli;help;simulation' {
            [CompletionResult]::new('list', 'list', [CompletionResultType]::ParameterValue, 'List simulations')
            [CompletionResult]::new('get', 'get', [CompletionResultType]::ParameterValue, 'Get simulation')
            [CompletionResult]::new('set', 'set', [CompletionResultType]::ParameterValue, 'Set simulation parameters')
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Export simulation')
            [CompletionResult]::new('import', 'import', [CompletionResultType]::ParameterValue, 'Import simulation')
            break
        }
        'fujicli;help;simulation;list' {
            break
        }
        'fujicli;help;simulation;get' {
            break
        }
        'fujicli;help;simulation;set' {
            break
        }
        'fujicli;help;simulation;export' {
            break
        }
        'fujicli;help;simulation;import' {
            break
        }
        'fujicli;help;backup' {
            [CompletionResult]::new('export', 'export', [CompletionResultType]::ParameterValue, 'Export backup')
            [CompletionResult]::new('inspect', 'inspect', [CompletionResultType]::ParameterValue, 'Inspect and validate a backup artifact without connecting a camera')
            [CompletionResult]::new('import', 'import', [CompletionResultType]::ParameterValue, 'Import backup')
            break
        }
        'fujicli;help;backup;export' {
            break
        }
        'fujicli;help;backup;inspect' {
            break
        }
        'fujicli;help;backup;import' {
            break
        }
        'fujicli;help;image' {
            [CompletionResult]::new('render', 'render', [CompletionResultType]::ParameterValue, 'Render image')
            [CompletionResult]::new('recover', 'recover', [CompletionResultType]::ParameterValue, 'Recover a retained rendered JPEG by its camera object handle')
            break
        }
        'fujicli;help;image;render' {
            break
        }
        'fujicli;help;image;recover' {
            break
        }
        'fujicli;help;help' {
            break
        }
    })

    $completions.Where{ $_.CompletionText -like "$wordToComplete*" } |
        Sort-Object -Property ListItemText
}
