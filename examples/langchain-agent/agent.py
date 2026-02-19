"""
LangChain conversational agent with Cortex memory.

Usage:
    cortex serve &
    export OPENAI_API_KEY=sk-...
    python agent.py
"""

from langchain.memory import BaseMemory
from langchain_openai import ChatOpenAI
from langchain.chains import ConversationChain
from cortex_memory import Cortex
from typing import Any, Dict, List


class CortexMemory(BaseMemory):
    """LangChain-compatible memory backed by a Cortex knowledge graph."""

    def __init__(self, agent_id: str, cortex_addr: str = "localhost:9090"):
        self.agent_id = agent_id
        self.cx = Cortex(cortex_addr)

    @property
    def memory_variables(self) -> List[str]:
        return ["history"]

    def load_memory_variables(self, inputs: Dict[str, Any]) -> Dict[str, Any]:
        briefing = self.cx.briefing(self.agent_id)
        return {"history": briefing if briefing.strip() else "No previous context."}

    def save_context(self, inputs: Dict[str, Any], outputs: Dict[str, str]) -> None:
        user_input = inputs.get("input", "")
        ai_output = outputs.get("response", "")
        self.cx.store(
            kind="event",
            title=f"Conversation: {user_input[:60]}",
            body=f"User: {user_input}\nAssistant: {ai_output}",
            source_agent=self.agent_id,
            tags=["conversation"],
            importance=0.5,
        )

    def clear(self) -> None:
        pass  # Cortex manages retention via policies


def main():
    agent_id = "langchain-demo"
    memory = CortexMemory(agent_id)
    llm = ChatOpenAI(model="gpt-4o", temperature=0.7)
    chain = ConversationChain(llm=llm, memory=memory, verbose=False)

    print("Cortex-powered LangChain agent. Type 'exit' to quit.\n")
    while True:
        user_input = input("You: ").strip()
        if user_input.lower() in ("exit", "quit", "q"):
            break
        if not user_input:
            continue
        response = chain.predict(input=user_input)
        print(f"Assistant: {response}\n")


if __name__ == "__main__":
    main()
