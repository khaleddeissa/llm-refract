"""A brief thank-you for people using Refract in a terminal."""

import os
import sys

COMMUNITY_MESSAGE = (
    "    /\\\n"
    "   /  \\\n"
    "  / /\\ \\   REFRACT\n"
    "  \\ \\/ /\n"
    "   \\  /\n"
    "    \\/\n\n"
    "Thanks for using Refract! If it helps you, a GitHub star or feedback would be appreciated.\n"
    "Star: https://github.com/khaleddeissa/llm-refract\n"
    "Feedback: https://github.com/khaleddeissa/llm-refract/issues"
)
_shown = False


def show_community_message() -> None:
    """Print once per process on terminal stderr; REFRACT_NO_BANNER=1 silences it."""
    global _shown
    if _shown or os.environ.get("REFRACT_NO_BANNER") == "1":
        return
    if sys.stderr is None or not sys.stderr.isatty():
        return
    try:
        print(COMMUNITY_MESSAGE, file=sys.stderr)
    except OSError:
        return
    _shown = True
