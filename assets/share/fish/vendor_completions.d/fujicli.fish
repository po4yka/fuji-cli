# Print an optspec for argparse to handle cmd's options that are independent of any subcommand.
function __fish_fujicli_global_optspecs
    string join \n v/verbose h/help V/version
end

function __fish_fujicli_needs_command
    # Figure out if the current invocation already has a command.
    set -l cmd (commandline -opc)
    set -e cmd[1]
    argparse -s (__fish_fujicli_global_optspecs) -- $cmd 2>/dev/null
    or return
    if set -q argv[1]
        # Also print the command, so this can be used to figure out what it is.
        echo $argv[1]
        return 1
    end
    return 0
end

function __fish_fujicli_using_subcommand
    set -l cmd (__fish_fujicli_needs_command)
    test -z "$cmd"
    and return 1
    contains -- $cmd[1] $argv
end

complete -c fujicli -n "__fish_fujicli_needs_command" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_needs_command" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fujicli -n "__fish_fujicli_needs_command" -s V -l version -d 'Print version'
complete -c fujicli -n "__fish_fujicli_needs_command" -f -a "device" -d 'Manage devices'
complete -c fujicli -n "__fish_fujicli_needs_command" -f -a "simulation" -d 'Manage film simulations'
complete -c fujicli -n "__fish_fujicli_needs_command" -f -a "backup" -d 'Manage backups'
complete -c fujicli -n "__fish_fujicli_needs_command" -f -a "image" -d 'Manage and render images'
complete -c fujicli -n "__fish_fujicli_needs_command" -f -a "completion" -d 'Generate a shell completion script'
complete -c fujicli -n "__fish_fujicli_needs_command" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and not __fish_seen_subcommand_from list info help" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and not __fish_seen_subcommand_from list info help" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and not __fish_seen_subcommand_from list info help" -f -a "list" -d 'List cameras'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and not __fish_seen_subcommand_from list info help" -f -a "info" -d 'Get camera info'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and not __fish_seen_subcommand_from list info help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from list" -s j -l json -d 'Format output using JSON'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from list" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from info" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from info" -l emulate -d 'Treat device as a different model using <VENDOR_ID>:<PRODUCT_ID>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from info" -s j -l json -d 'Format output using JSON'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from info" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from info" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from help" -f -a "list" -d 'List cameras'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from help" -f -a "info" -d 'Get camera info'
complete -c fujicli -n "__fish_fujicli_using_subcommand device; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and not __fish_seen_subcommand_from list get set export import help" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and not __fish_seen_subcommand_from list get set export import help" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and not __fish_seen_subcommand_from list get set export import help" -f -a "list" -d 'List simulations'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and not __fish_seen_subcommand_from list get set export import help" -f -a "get" -d 'Get simulation'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and not __fish_seen_subcommand_from list get set export import help" -f -a "set" -d 'Set simulation parameters'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and not __fish_seen_subcommand_from list get set export import help" -f -a "export" -d 'Export simulation'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and not __fish_seen_subcommand_from list get set export import help" -f -a "import" -d 'Import simulation'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and not __fish_seen_subcommand_from list get set export import help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from list" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from list" -s j -l json -d 'Format output using JSON'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from list" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from list" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from get" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from get" -s j -l json -d 'Format output using JSON'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from get" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from get" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l clarity -d 'Clarity' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l color -d 'Color' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l color-chrome-effect -d 'Color Chrome Effect' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l color-chrome-fx-blue -d 'Color Chrome FX Blue' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l color-space -d 'Color Space' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l custom-setting-name -d 'Custom Setting Name' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l dynamic-range -d 'Dynamic Range' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l dynamic-range-priority -d 'Dynamic Range Priority' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l film-simulation -d 'Film Simulation' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l grain-effect -d 'Grain Effect' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l highlight-tone -d 'Highlight Tone' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l image-quality -d 'Image Quality' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l image-size -d 'Image Size' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l lens-modulation-optimizer -d 'Lens Modulation Optimizer' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l monochromatic-color-temperature -d 'Monochromatic Color Temperature' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l monochromatic-color-tint -d 'Monochromatic Color Tint' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l noise-reduction -d 'High ISO Noise Reduction' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l shadow-tone -d 'Shadow Tone' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l sharpness -d 'Sharpness' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l smooth-skin-effect -d 'Smooth Skin Effect' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l white-balance -d 'White Balance' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l white-balance-shift-blue -d 'White Balance Shift Blue' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l white-balance-shift-red -d 'White Balance Shift Red' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l white-balance-temperature -d 'White Balance Temperature' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -l target-serial-sha256 -d 'SHA-256 fingerprint of the exact physical camera serial number' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -s j -l json -d 'Format output using JSON'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from set" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from export" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from export" -l force -d 'Replace an existing regular output file'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from export" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from export" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from import" -l target-serial-sha256 -d 'SHA-256 fingerprint of the exact physical camera serial number' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from import" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from import" -s j -l json -d 'Format output using JSON'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from import" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from import" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from help" -f -a "list" -d 'List simulations'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from help" -f -a "get" -d 'Get simulation'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from help" -f -a "set" -d 'Set simulation parameters'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from help" -f -a "export" -d 'Export simulation'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from help" -f -a "import" -d 'Import simulation'
complete -c fujicli -n "__fish_fujicli_using_subcommand simulation; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and not __fish_seen_subcommand_from export inspect import help" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and not __fish_seen_subcommand_from export inspect import help" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and not __fish_seen_subcommand_from export inspect import help" -f -a "export" -d 'Export backup'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and not __fish_seen_subcommand_from export inspect import help" -f -a "inspect" -d 'Inspect and validate a backup artifact without connecting a camera'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and not __fish_seen_subcommand_from export inspect import help" -f -a "import" -d 'Import backup'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and not __fish_seen_subcommand_from export inspect import help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from export" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from export" -l force -d 'Replace an existing regular output file'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from export" -s j -l json -d 'Format output using JSON'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from export" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from export" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from inspect" -s j -l json -d 'Format output using JSON'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from inspect" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from inspect" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -l recovery-backup -d 'New file that receives the target\'s current settings before restore' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -l expect-sha256 -d 'Expected SHA-256 of the complete input artifact' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -l target-serial-sha256 -d 'Expected SHA-256 fingerprint of a different target camera serial' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -l yes -d 'Confirm sending the validated backup artifact to the selected camera'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -l dry-run -d 'Validate compatibility without exporting recovery state or restoring'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -l allow-stdin -d 'Permit destructive import from stdin (also requires --expect-sha256)'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -s j -l json -d 'Format dry-run output using JSON'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from import" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from help" -f -a "export" -d 'Export backup'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from help" -f -a "inspect" -d 'Inspect and validate a backup artifact without connecting a camera'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from help" -f -a "import" -d 'Import backup'
complete -c fujicli -n "__fish_fujicli_using_subcommand backup; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and not __fish_seen_subcommand_from render recover help" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and not __fish_seen_subcommand_from render recover help" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and not __fish_seen_subcommand_from render recover help" -f -a "render" -d 'Render image'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and not __fish_seen_subcommand_from render recover help" -f -a "recover" -d 'Recover a retained rendered JPEG by its camera object handle'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and not __fish_seen_subcommand_from render recover help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l target-serial-sha256 -d 'SHA-256 fingerprint of the exact physical camera serial number' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l simulation-file -d 'Path to exported simulation file' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l clarity -d 'Clarity' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l color -d 'Color' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l color-chrome-effect -d 'Color Chrome Effect' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l color-chrome-fx-blue -d 'Color Chrome FX Blue' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l color-space -d 'Color Space' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l dynamic-range -d 'Dynamic Range' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l dynamic-range-priority -d 'Dynamic Range Priority' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l exposure-offset -d 'Exposure Offset' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l file-type -d 'File Type' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l film-simulation -d 'Film Simulation' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l grain-effect -d 'Grain Effect' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l highlight-tone -d 'Highlight Tone' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l image-quality -d 'Image Quality' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l image-size -d 'Image Size' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l lens-modulation-optimizer -d 'Lens Modulation Optimizer' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l monochromatic-color-temperature -d 'Monochromatic Color Temperature' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l monochromatic-color-tint -d 'Monochromatic Color Tint' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l noise-reduction -d 'High ISO Noise Reduction' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l shadow-tone -d 'Shadow Tone' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l sharpness -d 'Sharpness' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l smooth-skin-effect -d 'Smooth Skin Effect' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l teleconverter -d 'Teleconverter' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l white-balance -d 'White Balance' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l white-balance-shift-blue -d 'White Balance Shift Blue' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l white-balance-shift-red -d 'White Balance Shift Red' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l white-balance-temperature -d 'White Balance Temperature' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l draft -d 'Render a lower-quality (faster) preview'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -l force -d 'Replace an existing regular output file'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from render" -s h -l help -d 'Print help (see more with \'--help\')'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from recover" -l target-serial-sha256 -d 'SHA-256 fingerprint of the exact physical camera serial number' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from recover" -s d -l device -d 'Manually specify target device using USB <BUS>.<ADDRESS>' -r
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from recover" -l force -d 'Replace an existing regular output file'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from recover" -l delete-after-save -d 'Delete the camera object after a verified local file save'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from recover" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from recover" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from help" -f -a "render" -d 'Render image'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from help" -f -a "recover" -d 'Recover a retained rendered JPEG by its camera object handle'
complete -c fujicli -n "__fish_fujicli_using_subcommand image; and __fish_seen_subcommand_from help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand completion" -s v -l verbose -d 'Log extra debugging information (multiple instances increase verbosity)'
complete -c fujicli -n "__fish_fujicli_using_subcommand completion" -s h -l help -d 'Print help'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and not __fish_seen_subcommand_from device simulation backup image completion help" -f -a "device" -d 'Manage devices'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and not __fish_seen_subcommand_from device simulation backup image completion help" -f -a "simulation" -d 'Manage film simulations'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and not __fish_seen_subcommand_from device simulation backup image completion help" -f -a "backup" -d 'Manage backups'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and not __fish_seen_subcommand_from device simulation backup image completion help" -f -a "image" -d 'Manage and render images'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and not __fish_seen_subcommand_from device simulation backup image completion help" -f -a "completion" -d 'Generate a shell completion script'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and not __fish_seen_subcommand_from device simulation backup image completion help" -f -a "help" -d 'Print this message or the help of the given subcommand(s)'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from device" -f -a "list" -d 'List cameras'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from device" -f -a "info" -d 'Get camera info'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from simulation" -f -a "list" -d 'List simulations'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from simulation" -f -a "get" -d 'Get simulation'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from simulation" -f -a "set" -d 'Set simulation parameters'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from simulation" -f -a "export" -d 'Export simulation'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from simulation" -f -a "import" -d 'Import simulation'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from backup" -f -a "export" -d 'Export backup'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from backup" -f -a "inspect" -d 'Inspect and validate a backup artifact without connecting a camera'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from backup" -f -a "import" -d 'Import backup'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from image" -f -a "render" -d 'Render image'
complete -c fujicli -n "__fish_fujicli_using_subcommand help; and __fish_seen_subcommand_from image" -f -a "recover" -d 'Recover a retained rendered JPEG by its camera object handle'
