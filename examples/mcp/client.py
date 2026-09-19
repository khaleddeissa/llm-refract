"""Initialize a real MCP session and read service capabilities."""

import asyncio
import sys

from mcp.client.stdio import stdio_client

from mcp import ClientSession, StdioServerParameters


async def main():
    params = StdioServerParameters(command=sys.executable, args=["-m", "refract_mcp"])
    async with stdio_client(params) as (read, write), ClientSession(read, write) as session:
        await session.initialize()
        print([t.name for t in (await session.list_tools()).tools])
        print((await session.read_resource("refract://capabilities")).contents)


asyncio.run(main())
