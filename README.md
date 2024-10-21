# hotkeyd
a super simple daemon that can listen for certain keystrokes and perform actions.

tested on an m2 macbook pro on sequoia 15.0 (24A335).

## goals
1. when a set of keypresses is detected we should be able to configure the following things to happen:
  - run a script
  - run a different set of keypresses
2. if we have a non boolean event (such as a mouse move or scroll) that includes things like speed, location, etc. we want to be able to be able to configure a modification to it. for example: if we hold shift while scrolling we want to be able to multiply the scroll speed by X
3. the configuration should be type checked and we should be able to detect if it is invalid.
4. we should be able to load a new config without restarting the daemon
5. whitelist/blacklist apps from certain macros actions

## types of macros
*key/button*: when a certain set of keys/buttons are pressed down at the same time, do another action instead.
*scroll modifier*: when a certain set of keys are pressed down while also scrolling. receive the current scroll values and do certain actions while also having delta_x and delta_y.
*mouse modifier*: when a certain set of keys are pressed down while also moving the mouse. receive the current mouse position and do certain actions while also having x and y. (maybe we should also include direction of movement and also screen size)

## stuff to fix
so currently in order to get the correct perms on macos to be able to capture keystrokes we need to allow it in system settings > privacy & security > accesibility. however the annoying thing is that we cannnot directly add the binary to the permissions page. for some reason we need to make a "launcher" binary who's only job is to spwan our actual binary and give the launcher binary the permissions for the accesibility api. not really sure if im missing something here or this is the correct way to do it but i just made a quick launcher in go that basically launches our real binary and pipes stdout and stderr thru so we can still see logsi.

## other
im going to be trying out [tigerstyle](https://github.com/tigerbeetle/tigerbeetle/blob/main/docs/TIGER_STYLE.md) while building this.
