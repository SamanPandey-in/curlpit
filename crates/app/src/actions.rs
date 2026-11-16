//! The keyboard-dispatched actions, declared on their own so that `main` (which binds
//! the keys) and the render modules (which handle them) can both name them without
//! depending on each other.

use gpui::actions;

actions!(
    api_tester,
    [
        SendRequest,
        SaveRequest,
        NewTab,
        CloseTab,
        ReopenTab,
        FindInResponse
    ]
);
