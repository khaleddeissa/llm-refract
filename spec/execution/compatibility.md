# Compatibility

Version changes require shared fixture tests across Rust, Python and TypeScript.
Unknown object fields are currently tolerated; unknown event/policy enums and spec versions are rejected.
Keep stored artifacts immutable; migrate into a new artifact rather than rewriting evidence.
