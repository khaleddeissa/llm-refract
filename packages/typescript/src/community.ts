/** A brief thank-you for people using Refract in a terminal. */
const communityMessage =
  "    /\\\n" +
  "   /  \\\n" +
  "  / /\\ \\   REFRACT\n" +
  "  \\ \\/ /\n" +
  "   \\  /\n" +
  "    \\/\n\n" +
  "Thanks for using Refract! If it helps you, a GitHub star or feedback would be appreciated.\n" +
  "Star: https://github.com/khaleddeissa/llm-refract\n" +
  "Feedback: https://github.com/khaleddeissa/llm-refract/issues";
let shown = false;

/** Print once per process on terminal stderr; REFRACT_NO_BANNER=1 silences it. */
export function showCommunityMessage(): void {
  if (shown || process.env.REFRACT_NO_BANNER === "1" || !process.stderr.isTTY)
    return;
  shown = true;
  console.error(communityMessage);
}
