#!/usr/bin/env python3
"""
Concurrent EVM RPC Endpoint Health Checker
Usage: python3 scripts/test-rpc-endpoints.py [config/rpc_endpoints.json] [--concurrency 20]
"""

import sys
import json
import time
import asyncio
import urllib.request
import urllib.error
from typing import Dict, Any, List

DEFAULT_FILE = "config/rpc_endpoints.json"
DEFAULT_CONCURRENCY = 20
TIMEOUT_SECONDS = 5.0

async def check_rpc_endpoint(sem: asyncio.Semaphore, endpoint: Dict[str, Any], opener: urllib.request.OpenerDirector = None) -> Dict[str, Any]:
    url = endpoint.get("url", "").strip()
    chain_id = endpoint.get("chain_id")
    chain_name = endpoint.get("chain_name") or f"Chain {chain_id}"
    
    result = {
        "chain_id": chain_id,
        "chain_name": chain_name,
        "url": url,
        "weight": endpoint.get("weight", 100),
        "status": "FAIL",
        "latency_ms": None,
        "block_number": None,
        "error": None
    }
    
    if not url.startswith("http://") and not url.startswith("https://"):
        result["error"] = "Invalid URL protocol"
        return result

    req_data = json.dumps({
        "jsonrpc": "2.0",
        "method": "eth_blockNumber",
        "params": [],
        "id": 1
    }).encode("utf-8")

    async with sem:
        loop = asyncio.get_running_loop()
        start = time.perf_counter()
        
        def do_request():
            req = urllib.request.Request(
                url,
                data=req_data,
                headers={"Content-Type": "application/json", "User-Agent": "EVMEventLake-RPC-Checker/1.0"}
            )
            if opener is not None:
                with opener.open(req, timeout=TIMEOUT_SECONDS) as response:
                    return response.read()
            else:
                with urllib.request.urlopen(req, timeout=TIMEOUT_SECONDS) as response:
                    return response.read()

        try:
            raw_response = await loop.run_in_executor(None, do_request)
            elapsed = (time.perf_counter() - start) * 1000
            
            resp_json = json.loads(raw_response.decode("utf-8"))
            if "result" in resp_json and resp_json["result"]:
                block_hex = resp_json["result"]
                block_num = int(block_hex, 16)
                result["status"] = "OK"
                result["latency_ms"] = round(elapsed, 1)
                result["block_number"] = block_num
            elif "error" in resp_json:
                result["error"] = resp_json["error"].get("message", "JSON-RPC error")
            else:
                result["error"] = "No result in response"
        except Exception as e:
            result["error"] = str(e)
            
    return result

async def main():
    file_path = DEFAULT_FILE
    concurrency = DEFAULT_CONCURRENCY
    no_proxy = False
    
    args = sys.argv[1:]
    i = 0
    while i < len(args):
        arg = args[i]
        if arg == "--no-proxy":
            no_proxy = True
        elif arg == "--concurrency" and i + 1 < len(args):
            concurrency = int(args[i + 1])
            i += 1
        elif not arg.startswith("--"):
            file_path = arg
        i += 1

    opener = None
    if no_proxy:
        import os
        for k in ["http_proxy", "https_proxy", "all_proxy", "HTTP_PROXY", "HTTPS_PROXY", "ALL_PROXY"]:
            if k in os.environ:
                del os.environ[k]
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
        
    updated_at_str = None
    try:
        with open(file_path, "r", encoding="utf-8") as f:
            data = json.load(f)
            if isinstance(data, dict) and "endpoints" in data:
                endpoints = data["endpoints"]
                updated_at_str = data.get("updated_at")
            elif isinstance(data, list):
                endpoints = data
            else:
                print(f"Error: Invalid JSON format in {file_path}")
                sys.exit(1)
    except FileNotFoundError:
        print(f"File not found: {file_path}")
        print("Tip: Provide an example JSON file like: python3 scripts/test-rpc-endpoints.py config/rpc_endpoints.json.example")
        sys.exit(1)
    except Exception as e:
        print(f"Error reading {file_path}: {e}")
        sys.exit(1)

    proxy_info = " [DIRECT / NO PROXY]" if no_proxy else ""
    time_info = f" (Updated: {updated_at_str})" if updated_at_str else ""
    print(f"\n🚀 Concurrently testing {len(endpoints)} RPC endpoints{time_info}{proxy_info} with concurrency={concurrency} (timeout={TIMEOUT_SECONDS}s)...\n")
    
    sem = asyncio.Semaphore(concurrency)
    tasks = [check_rpc_endpoint(sem, ep, opener) for ep in endpoints]
    results = await asyncio.gather(*tasks)
    
    # Sort by chain_id, status (OK first), latency_ms
    results.sort(key=lambda x: (x["chain_id"] or 0, 0 if x["status"] == "OK" else 1, x["latency_ms"] or 99999))
    
    # Summary Table
    print(f"{'Chain ID':<10} {'Chain Name':<16} {'Status':<8} {'Latency':<10} {'Block #':<12} {'URL'}")
    print("-" * 90)
    
    ok_count = 0
    fail_count = 0
    for r in results:
        status_str = f"\033[92m{r['status']}\033[0m" if r['status'] == "OK" else f"\033[91m{r['status']}\033[0m"
        latency_str = f"{r['latency_ms']} ms" if r['latency_ms'] is not None else "-"
        block_str = str(r['block_number']) if r['block_number'] is not None else "-"
        print(f"{r['chain_id']:<10} {r['chain_name'][:15]:<16} {status_str:<17} {latency_str:<10} {block_str:<12} {r['url']}")
        if r["status"] == "OK":
            ok_count += 1
        else:
            fail_count += 1
            if r["error"]:
                print(f"  └── ⚠️  Error: {r['error']}")
                
    print("-" * 90)
    print(f"✅ Total: {len(results)} | Healthy: {ok_count} | Failed: {fail_count}\n")

if __name__ == "__main__":
    asyncio.run(main())
