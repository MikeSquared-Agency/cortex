"""
Personal assistant with persistent Cortex memory.

Remembers facts, decisions, and preferences across sessions.

Usage:
    cortex serve &
    export OPENAI_API_KEY=sk-...
    python assistant.py
"""

from openai import OpenAI
from cortex_memory import Cortex

cx = Cortex("localhost:9090")
llm = OpenAI()
AGENT_ID = "personal-assistant"


def get_system_prompt() -> str:
    briefing = cx.briefing(AGENT_ID)
    base = (
        "You are a helpful personal assistant with long-term memory. "
        "You remember facts, decisions, and preferences the user has shared. "
        "When the user tells you something important, confirm you've stored it. "
        "When asked about past context, use your briefing."
    )
    if briefing.strip():
        return f"{base}\n\n## Your Memory\n{briefing}"
    return base


def store_if_important(user_msg: str, assistant_reply: str):
    """Store interactions that seem to contain important information."""
    indicators = ["remember", "important", "decided", "prefer", "always", "never", "fact:"]
    if any(word in user_msg.lower() for word in indicators):
        cx.store(
            kind="event",
            title=user_msg[:80],
            body=f"User: {user_msg}\nAssistant: {assistant_reply}",
            source_agent=AGENT_ID,
            importance=0.7,
            tags=["user-instruction"],
        )


def chat(history: list, user_msg: str) -> str:
    history.append({"role": "user", "content": user_msg})
    response = llm.chat.completions.create(
        model="gpt-4o",
        messages=[{"role": "system", "content": get_system_prompt()}] + history,
    )
    reply = response.choices[0].message.content
    history.append({"role": "assistant", "content": reply})
    store_if_important(user_msg, reply)
    return reply


def main():
    print("Personal Assistant (Cortex memory). Type 'exit' to quit.\n")
    briefing = cx.briefing(AGENT_ID)
    if briefing.strip():
        print("(Memory loaded from previous sessions)\n")

    history = []
    while True:
        try:
            user_input = input("You: ").strip()
        except (EOFError, KeyboardInterrupt):
            print("\nGoodbye!")
            break
        if user_input.lower() in ("exit", "quit"):
            break
        if not user_input:
            continue
        reply = chat(history, user_input)
        print(f"Assistant: {reply}\n")


if __name__ == "__main__":
    main()
