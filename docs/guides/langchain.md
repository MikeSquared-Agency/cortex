# LangChain Integration

Use Cortex as the memory backend for a LangChain agent.

## Install

```bash
pip install cortex-memory langchain langchain-openai
```

## Memory Class

```python
from langchain.memory import BaseMemory
from cortex_memory import Cortex
from typing import Any, Dict, List

class CortexMemory(BaseMemory):
    def __init__(self, agent_id: str, cortex_addr: str = "localhost:9090"):
        self.agent_id = agent_id
        self.cx = Cortex(cortex_addr)

    @property
    def memory_variables(self) -> List[str]:
        return ["history"]

    def load_memory_variables(self, inputs: Dict[str, Any]) -> Dict[str, Any]:
        briefing = self.cx.briefing(self.agent_id)
        return {"history": briefing}

    def save_context(self, inputs: Dict[str, Any], outputs: Dict[str, str]) -> None:
        user_input = inputs.get("input", "")
        ai_output = outputs.get("output", "")
        self.cx.store(
            kind="event",
            title=f"User: {user_input[:80]}",
            body=f"User: {user_input}\nAssistant: {ai_output}",
            source_agent=self.agent_id,
            tags=["conversation"],
            importance=0.6,
        )

    def clear(self) -> None:
        pass  # Cortex manages its own retention
```

## Usage

```python
from langchain_openai import ChatOpenAI
from langchain.chains import ConversationChain

memory = CortexMemory("my-agent")
llm = ChatOpenAI(model="gpt-4o")
chain = ConversationChain(llm=llm, memory=memory)

response = chain.predict(input="What do you know about our API?")
```

## Full Example

See [`examples/langchain-agent/`](../../examples/langchain-agent/) in the repository.
