# observations

the current design at 3a45c8d06fb518048e8504ae51e0309a430f6e1d has some things that were overlooked

## events from OS
originally i made the assumption that all keys would behave the same in terms of receiving events from the OS. this is not the case. modifier keys only emit events when they are explicitly changed. for example, if i hold down the shift key i will receive 1 event no matter how long the key is held down for. this is different compared to non-modifier keys. when holding down a non-modifier key we receive keypress events constantly. more specificially we will receive `KeyPress` constantly and then eventually when the key is released we will receive the `KeyRelease`.

## issues with macros
there is currently a bug that basically causes the buttons used in the macro to not be blocked. currently i think there's 2 different things that could be to blame: the way we are storing the state or the way that we block events. after some further investigation however im more inclined to believe that its a symptom of the way that im doing event blocking. this is also actually caused by our initial assumption from the previous issue that all keys were going to be handled exactly the same. for example, lets say that we want to map `shift + /` to run `:wa + enter` (saving all buffers in nvim). when we attempt to trigger the bind the following is the order of the events as they come in:

1. shift event
2. / event

since we dont know that we want to block the shift key until we receive the / key we dont block it and only block the / key. this means when we attempt to run the macro the shift key is still held down and it ends up running `:WA + enter` because the shift modified the `a` and `a` keys to be capital which is wrong.

im thinking the best way to solve this problem is by decouple blocking from the callback we use to receive the event. this means we never block via the callback and instead have to find another method of blocking.

we could try and simulate a block by just negating the keys used by the currently running macro. for example, in the example above we wouldn't block the `/` or `shift` keys and instead once we see that we received a key event that matches a bind we can then send `KeyRelease` for both `shift` and `/` before running the macro. something to test is if we send the release event for shift while the user is holding the shift key down will we immediately receive a press event for the shift key from the OS or not?

## some solutions
given these observations i think a productive change wouold be to complete rethink the way we are storing the keyboard state. modifiers would actually benefit from the current system of just simply storing what keys are currently pressed down. however when dealing with non-modifier keys it might make more sense to handle them as more of a stream.
 
we also may have to make some alterations to the way that we parse the configuration file because we essentially need to require exactly 1 non-modifier key and at least 1 modifier key per each bind. this is because if we are going to adopt the above method of storing the keyboard state we would not be able to handle 2 non-modifier keys being pressed since they would be handled one at a time.
