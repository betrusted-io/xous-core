# An App Loader for Xous

This app allows you to load apps onto your device at runtime and is intended to be used by developers who would like to quickly test their apps on physical hardware.
At the moment the apps are loaded onto the device using a primitive HTTP server; however, in the future, other (faster) methods may be implemented.
Depending on how good the device's WiFi connection is, loading the app can take anywhere from 20 seconds for a small app to a couple of minutes for a large app. Additionally, if you are loading a particularly large app, your device's WiFi connection may cut out while you are trying to load it, leading to your device not being able to download the app (this is another reason why different methods of app loading may need to be implemented).
Note that at the moment you can only load apps once each time the device runs since process destruction has not been implemented in Xous yet.

## Usage

First, add the app loader to your device's image as you would with any other app:
```
$ cargo xtask app-image-xip --app app-loader
$ sudo python3 tools/usb_update.py -k target/riscv32imac-unknown-xous-elf/release/xous.img -l target/riscv32imac-unknown-xous-elf/release/loader.bin
```
Then, compile the apps you would like on your device and start the app server. For example, if you would like to serve the apps `hello`, `ball`, and `vault` on port 8000, run:
```
$ cargo xtask compile-apps --app hello --app ball --app vault
$ python3 tools/app_server.py hello ball vault -p 8000
```
Now, after selecting `App Loader` from the app menu on the device, you should see a menu that looks like:
```
/------------\
| Set Server |
| Close      |
\------------/
```
From here, you can select `Set Server` and then input the address of your server (e.g., `http://` followed by the ip address of your server, a `:`, and the port you are running it on). If you do not prefix the address with `http://`, the app will present an error and make you re-enter it.
Now, the screen should look like:
```
/-----------------\
| Add App         |
| Reload App List |
| Set Server      |
| Close           |
\-----------------/
```
If you select `Add App`, this should bring up a submenu with a list of the apps served on your app server. Selecting one of these apps will load it onto the device. The app you load will now show up in the app loader menu. For example, if you loaded `Hello World`, the menu should look like:
```
/-----------------\
| Hello World     |
| Add App         |
| Reload App List |
| Set Server      |
| Close           |
\-----------------/
```
Clicking on `Hello World` (or the name of some other app you loaded) should now open the app.
