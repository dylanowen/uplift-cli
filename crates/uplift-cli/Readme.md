# uplift-cli

A CLI for controlling Uplift standing desks over Bluetooth.

## Install

```sh
cargo install uplift-cli
```

## Commands

```sh
# Get the current desk height
uplift query

# Move to sitting position
uplift sit

# Save current height as sitting position
uplift sit save

# Move to standing position
uplift stand

# Save current height as standing position
uplift stand save

# Toggle between sit and stand based on current height
uplift toggle

# Retry sit until the desk reaches the target height
uplift force-sit

# Retry stand until the desk reaches the target height
uplift force-stand

# Retry toggle until the desk reaches the target height
uplift force-toggle

# Stream height updates continuously
uplift listen
```