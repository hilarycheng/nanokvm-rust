# AIM

* Rewrite the NanoKVM TypeScript application in Rust.
* Use egui/eframe for the desktop UI instead of an HTML-based UI.
* Capture the full screen or a user-selected area and save it as WebP or PNG.
* Allow full-screen capture from the toolbar while Focus Mode is active.
* Start partial capture from the toolbar. Pause remote mouse forwarding while the user selects an area. Resume forwarding only if Focus Mode remains active after capture.
* Provide separate panels for standard V4L2 image controls and GPU controls. GPU controls include Gamma, Highlights, and Saturation. Users may adjust V4L2 gamma and GPU gamma together.
* Provide clear labels and guidance for both color-control panels.
* Allow users to capture and release remote keyboard and mouse input.
* In Focus Mode, capture keyboard input for the remote host instead of sending it to Sway. Reserve Right-Ctrl, Right-Alt as the local shortcut to release input.
* Provide a top toolbar styled like Microsoft Remote Desktop. Allow users to pin the toolbar or enable auto-hide.
* In auto-hide mode, keep a narrow strip visible. Reveal the toolbar when the pointer enters that strip.
* Allow users to switch between Focus Mode and Non-Focus Mode from the toolbar. Define Non-Focus Mode as normal VNC-style input handling.
* Provide Disconnect and Fullscreen buttons in the toolbar.
* Save and restore GPU color presets. Apply GPU color adjustments with wgpu/WGSL.
* On manual disconnect, stop video capture and serial I/O, clear local input state, and attempt to send neutral keyboard and mouse reports before closing serial.
* On unexpected V4L2 or serial disconnection, release available resources and clear local input state. Attempt HID release reports when serial is available. If a report fails, continue cleanup without blocking.
* Allow physical Meta to be forwarded to the remote host, or allow users to map another key to remote Meta.
