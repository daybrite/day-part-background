# Background Lab

Schedule a registered handler and inspect its saved completion count with
[day-part-background](../README.md).

From this directory:

```sh
day launch -t ios-uikit --script dayscript/demo.yaml
day launch -t android-material --script dayscript/demo.yaml
day launch -t macos-appkit --script dayscript/demo.yaml
```

On desktop, choose **Run in this process after 5 seconds**, wait, then **Refresh count**.
Closing the process discards pending in-process jobs. System-scheduling controls are disabled
when persistent scheduling is unavailable.

On iOS or Android, schedule OS work, leave the app, then return and refresh the count. The OS
chooses when to run it. **Run handler now** tests the handler directly and does not verify
persistent scheduling or process restart.

The portable walkthrough checks the UI, cancels a pending in-process job, and invokes the handler
where supported. Follow the [platform setup guide](../docs/background.md#platform-setup) for
native scheduling requirements. UI strings are localized in `resource/locales/`.
