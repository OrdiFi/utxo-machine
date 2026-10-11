use serde::Deserialize;
use serde_json::{json, Map, Value};
use std::{
    collections::BTreeMap,
    env, fs,
    process::{Command, ExitCode},
};

#[derive(Debug, Deserialize)]
struct UtxoContract {
    protocol: String,
    version: u32,

    #[serde(rename = "type")]
    contract_type: Option<String>,

    actions: BTreeMap<String, ContractAction>,
}

#[derive(Debug, Deserialize)]
struct ContractAction {
    operation: BceOperation,

    #[serde(default)]
    conditions: Vec<Value>,

    #[serde(default)]
    effects: Vec<Value>,

    #[serde(rename = "requiredSignatures", default)]
    required_signatures: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
enum BceOperation {
    Compose,
    Split,
    Extract,
    Insert,
}

impl BceOperation {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Compose => "compose",
            Self::Split => "split",
            Self::Extract => "extract",
            Self::Insert => "insert",
        }
    }
}

fn read_json(path: &str) -> Result<Value, String> {
    let raw = fs::read_to_string(path).map_err(|e| format!("cannot read JSON file {path}: {e}"))?;

    serde_json::from_str(&raw).map_err(|e| format!("invalid JSON in {path}: {e}"))
}

fn load_contract(path: &str) -> Result<(UtxoContract, Value), String> {
    let root = read_json(path)?;

    let contract: UtxoContract =
        serde_json::from_value(root.clone()).map_err(|e| format!("invalid UTXO contract: {e}"))?;

    if contract.protocol != "ordifi-utxo-contract" {
        return Err(format!("unsupported protocol: {}", contract.protocol));
    }

    if contract.version != 1 {
        return Err(format!("unsupported version: {}", contract.version));
    }

    Ok((contract, root))
}

fn parse_block_height(args: &[String]) -> Result<Option<u64>, String> {
    let mut i = 0;

    while i < args.len() {
        if args[i] == "--block-height" {
            let value = args
                .get(i + 1)
                .ok_or_else(|| "missing value for --block-height".to_string())?;

            return value
                .parse::<u64>()
                .map(Some)
                .map_err(|_| format!("invalid block height: {value}"));
        }

        i += 1;
    }

    Ok(None)
}

fn parse_context_path(args: &[String]) -> Result<Option<String>, String> {
    let mut i = 0;

    while i < args.len() {
        if args[i] == "--context" {
            let value = args
                .get(i + 1)
                .ok_or_else(|| "missing value for --context".to_string())?;

            return Ok(Some(value.clone()));
        }

        i += 1;
    }

    Ok(None)
}

fn parse_named_arg(args: &[String], name: &str) -> Result<Option<String>, String> {
    let mut i = 0;

    while i < args.len() {
        if args[i] == name {
            let value = args
                .get(i + 1)
                .ok_or_else(|| format!("missing value for {name}"))?;

            return Ok(Some(value.clone()));
        }

        i += 1;
    }

    Ok(None)
}

fn call_bce(binary: &str, command: &str, payload: &Value) -> Result<Value, String> {
    let payload_raw =
        serde_json::to_string(payload).map_err(|e| format!("cannot serialize BCE request: {e}"))?;

    let output = Command::new(binary)
        .arg(command)
        .arg(payload_raw)
        .output()
        .map_err(|e| format!("cannot execute BCE binary {binary}: {e}"))?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);
        return Err(format!("BCE {command} failed: {}", error.trim()));
    }

    serde_json::from_slice(&output.stdout)
        .map_err(|e| format!("invalid BCE {command} response: {e}"))
}

fn parse_bce_state_path(args: &[String]) -> Result<Option<String>, String> {
    let mut i = 0;

    while i < args.len() {
        if args[i] == "--bce-state" {
            let value = args
                .get(i + 1)
                .ok_or_else(|| "missing value for --bce-state".to_string())?;

            return Ok(Some(value.clone()));
        }

        i += 1;
    }

    Ok(None)
}

fn compile_extract_bce(execution_plan: &Value, current_state: &Value) -> Result<Value, String> {
    let outpoint = current_state
        .get("outpoint")
        .and_then(Value::as_str)
        .ok_or_else(|| "BCE state missing outpoint".to_string())?;

    let total_value = current_state
        .get("value")
        .and_then(Value::as_u64)
        .ok_or_else(|| "BCE state missing numeric value".to_string())?;

    let ordinals_address = current_state
        .get("address")
        .and_then(Value::as_str)
        .ok_or_else(|| "BCE state missing address".to_string())?;

    let satpoints = current_state
        .get("satpoints")
        .and_then(Value::as_array)
        .ok_or_else(|| "BCE state missing satpoints".to_string())?;

    let mut groups = Vec::<Value>::new();

    for satpoint in satpoints {
        let ids = satpoint
            .get("ids")
            .and_then(Value::as_array)
            .ok_or_else(|| "BCE satpoint missing ids".to_string())?;

        let offset = satpoint
            .get("offset")
            .and_then(Value::as_u64)
            .ok_or_else(|| "BCE satpoint missing offset".to_string())?;

        let postage = satpoint
            .get("postage")
            .and_then(Value::as_u64)
            .ok_or_else(|| "BCE satpoint missing postage".to_string())?;

        groups.push(json!({
            "ids": ids,
            "offset": offset,
            "postage": postage
        }));
    }

    let effects = execution_plan
        .get("effects")
        .and_then(Value::as_array)
        .ok_or_else(|| "execution plan missing effects".to_string())?;

    let mut selected: BTreeMap<String, (Value, String)> = BTreeMap::new();

    for effect in effects {
        if effect.get("type").and_then(Value::as_str) != Some("extract") {
            continue;
        }

        let id = effect
            .get("id")
            .and_then(Value::as_str)
            .ok_or_else(|| "extract effect missing id".to_string())?;

        let destination = effect
            .get("destination")
            .and_then(Value::as_str)
            .ok_or_else(|| "extract effect missing destination".to_string())?;

        let group = groups
            .iter()
            .find(|group| {
                group
                    .get("ids")
                    .and_then(Value::as_array)
                    .map(|ids| ids.iter().any(|candidate| candidate.as_str() == Some(id)))
                    .unwrap_or(false)
            })
            .ok_or_else(|| format!("extract id not found in BCE state: {id}"))?;

        let ids = group
            .get("ids")
            .and_then(Value::as_array)
            .ok_or_else(|| "BCE group missing ids".to_string())?
            .clone();

        let key =
            serde_json::to_string(&ids).map_err(|e| format!("cannot serialize BCE group: {e}"))?;

        if let Some((_, existing_destination)) = selected.get(&key) {
            if existing_destination != destination {
                return Err(format!(
                    "physically inseparable group has conflicting destinations: {:?}",
                    ids
                ));
            }
        } else {
            selected.insert(key, (Value::Array(ids), destination.to_string()));
        }
    }

    if selected.is_empty() {
        return Err("extract execution plan selected no BCE groups".to_string());
    }

    let extract_groups: Vec<Value> = selected.values().map(|(ids, _)| ids.clone()).collect();

    let extract_destinations: Vec<Value> = selected
        .values()
        .map(|(ids, destination)| {
            json!({
                "ids": ids,
                "address": destination
            })
        })
        .collect();

    let mut payment_outputs = Vec::<Value>::new();

    if let Some(requirements) = execution_plan.get("requirements").and_then(Value::as_array) {
        for requirement in requirements {
            if requirement.get("type").and_then(Value::as_str) != Some("payment") {
                continue;
            }

            let amount = requirement
                .get("amountSats")
                .and_then(Value::as_u64)
                .ok_or_else(|| "payment requirement missing amountSats".to_string())?;

            let to = requirement
                .get("to")
                .and_then(Value::as_str)
                .ok_or_else(|| "payment requirement missing destination".to_string())?;

            payment_outputs.push(json!({
                "address": to,
                "value": amount
            }));
        }
    }

    Ok(json!({
        "planRequest": {
            "input_utxo": outpoint,
            "ordinals_address": ordinals_address,
            "total_value": total_value,
            "groups": groups,
            "extract_groups": extract_groups
        },
        "extractDestinations": extract_destinations,
        "paymentOutputs": payment_outputs
    }))
}

fn lookup_path<'a>(root: &'a Value, path: &str) -> Option<&'a Value> {
    let mut current = root;

    for part in path.split('.') {
        current = current.get(part)?;
    }

    Some(current)
}

fn resolve_reference(
    value: &Value,
    contract_root: &Value,
    context_root: &Value,
    depth: usize,
) -> Result<Value, String> {
    if depth > 8 {
        return Err("reference resolution exceeded maximum depth".to_string());
    }

    let Some(text) = value.as_str() else {
        return Ok(value.clone());
    };

    /*
     * Special runtime identity of the currently executed contract.
     */
    if text == "contract" {
        if let Some(contract_id) = lookup_path(context_root, "contractId") {
            return Ok(contract_id.clone());
        }

        return Ok(value.clone());
    }

    /*
     * Runtime actor values.
     *
     * Example:
     * actor.ordinalsAddress
     * -> context.actor.ordinalsAddress
     */
    if text.starts_with("actor.") {
        return lookup_path(context_root, text)
            .cloned()
            .ok_or_else(|| format!("runtime reference not found: {text}"));
    }

    if text.contains('.') {
        /*
         * Direct contract reference.
         *
         * Example:
         * collateral.inscriptionId
         */
        if let Some(next) = lookup_path(contract_root, text) {
            if next != value {
                return resolve_reference(next, contract_root, context_root, depth + 1);
            }
        }

        /*
         * Party role reference.
         *
         * Example:
         * buyer.ordinalsAddress
         * -> parties.buyer.ordinalsAddress
         * -> actor.ordinalsAddress
         * -> runtime address
         */
        let party_path = format!("parties.{text}");

        if let Some(next) = lookup_path(contract_root, &party_path) {
            if next != value {
                return resolve_reference(next, contract_root, context_root, depth + 1);
            }
        }
    }

    Ok(value.clone())
}

fn resolve_object_fields(
    value: &Value,
    contract_root: &Value,
    context_root: &Value,
) -> Result<Value, String> {
    match value {
        Value::Object(map) => {
            let mut out = Map::new();

            for (key, value) in map {
                out.insert(
                    key.clone(),
                    resolve_object_fields(value, contract_root, context_root)?,
                );
            }

            Ok(Value::Object(out))
        }

        Value::Array(items) => {
            let mut out = Vec::new();

            for item in items {
                out.push(resolve_object_fields(item, contract_root, context_root)?);
            }

            Ok(Value::Array(out))
        }

        Value::String(_) => {
            let resolved = resolve_reference(value, contract_root, context_root, 0)?;

            if &resolved == value {
                Ok(resolved)
            } else {
                resolve_object_fields(&resolved, contract_root, context_root)
            }
        }

        _ => Ok(value.clone()),
    }
}

fn check_conditions(
    conditions: &[Value],
    block_height: Option<u64>,
    contract_root: &Value,
    context_root: &Value,
) -> Result<Vec<Value>, String> {
    let mut results = Vec::new();

    for condition in conditions {
        let condition_type = condition
            .get("type")
            .and_then(Value::as_str)
            .ok_or_else(|| "condition missing type".to_string())?;

        match condition_type {
            "block_height_min" => {
                let required = condition
                    .get("value")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "block_height_min missing numeric value".to_string())?;

                let current =
                    block_height.ok_or_else(|| "current block height required".to_string())?;

                if current < required {
                    return Err(format!(
                        "block height condition failed: current {current}, required >= {required}"
                    ));
                }

                results.push(json!({
                    "type": condition_type,
                    "required": required,
                    "current": current,
                    "passed": true
                }));
            }

            "block_height_max" => {
                let required = condition
                    .get("value")
                    .and_then(Value::as_u64)
                    .ok_or_else(|| "block_height_max missing numeric value".to_string())?;

                let current =
                    block_height.ok_or_else(|| "current block height required".to_string())?;

                if current > required {
                    return Err(format!(
                        "block height condition failed: current {current}, required <= {required}"
                    ));
                }

                results.push(json!({
                    "type": condition_type,
                    "required": required,
                    "current": current,
                    "passed": true
                }));
            }

            "payment" => {
                let resolved = resolve_object_fields(condition, contract_root, context_root)?;

                results.push(json!({
                    "type": "payment",
                    "status": "required_in_transaction",
                    "amountSats": resolved.get("amountSats"),
                    "from": resolved.get("from"),
                    "to": resolved.get("to")
                }));
            }

            "collateral_owner" => {
                let resolved = resolve_object_fields(condition, contract_root, context_root)?;

                results.push(json!({
                    "type": "collateral_owner",
                    "status": "requires_chain_check",
                    "inscriptionId": resolved.get("inscriptionId"),
                    "owner": resolved.get("owner")
                }));
            }

            _ => {
                return Err(format!("unsupported condition type: {condition_type}"));
            }
        }
    }

    Ok(results)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().collect();

    if args.len() < 3 {
        return Err(
            "usage: ordifi-machine <contract.json> <action> [--block-height N] [--context context.json] [--bce-state state.json] [--machine-outpoint TXID:VOUT] [--bce-bin PATH]"
                .to_string(),
        );
    }

    let (contract, contract_root) = load_contract(&args[1])?;
    let action_name = &args[2];

    let action = contract
        .actions
        .get(action_name)
        .ok_or_else(|| format!("unknown action: {action_name}"))?;

    let block_height = parse_block_height(&args[3..])?;

    let context_root = match parse_context_path(&args[3..])? {
        Some(path) => read_json(&path)?,
        None => json!({}),
    };

    let condition_results = check_conditions(
        &action.conditions,
        block_height,
        &contract_root,
        &context_root,
    )?;

    let resolved_effects = resolve_object_fields(
        &Value::Array(action.effects.clone()),
        &contract_root,
        &context_root,
    )?;

    let execution_plan = json!({
        "operation": action.operation.as_str(),
        "requirements": condition_results,
        "effects": resolved_effects,
        "requiredSignatures": action.required_signatures
    });

    let bce = match parse_bce_state_path(&args[3..])? {
        Some(path) => {
            if action.operation.as_str() != "extract" {
                return Err("--bce-state currently supports extract only".to_string());
            }

            let current_state = read_json(&path)?;
            Some(compile_extract_bce(&execution_plan, &current_state)?)
        }

        None => match parse_named_arg(&args[3..], "--machine-outpoint")? {
            Some(outpoint) => {
                if action.operation.as_str() != "extract" {
                    return Err("--machine-outpoint currently supports extract only".to_string());
                }

                let bce_bin = parse_named_arg(&args[3..], "--bce-bin")?
                    .or_else(|| env::var("ORDIFI_MACHINE_BCE_BIN").ok())
                    .unwrap_or_else(|| "bce-neutral-node".to_string());

                let current_state = call_bce(
                    &bce_bin,
                    "inspect-current-state",
                    &json!({ "outpoint": outpoint }),
                )?;

                let compiled = compile_extract_bce(&execution_plan, &current_state)?;

                let plan_request = compiled
                    .get("planRequest")
                    .cloned()
                    .ok_or_else(|| "compiled BCE request missing planRequest".to_string())?;

                let plan = call_bce(&bce_bin, "extract-plan", &plan_request)?;

                Some(json!({
                    "binary": bce_bin,
                    "currentState": current_state,
                    "compiled": compiled,
                    "plan": plan
                }))
            }

            None => None,
        },
    };

    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "ok": true,
            "contractType": contract.contract_type,
            "action": action_name,
            "executionPlan": execution_plan,
            "bce": bce
        }))
        .unwrap()
    );

    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,

        Err(error) => {
            eprintln!(
                "{}",
                serde_json::to_string_pretty(&json!({
                    "ok": false,
                    "error": error
                }))
                .unwrap()
            );

            ExitCode::FAILURE
        }
    }
}
