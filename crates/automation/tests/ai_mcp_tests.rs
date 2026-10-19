//! Integration tests ensuring AI Director commands are exposed and executable via MCP.

use filmcraft_automation::server::FilmcraftMcp;
use filmcraft_engine::Session;
use serde_json::json;

#[tokio::test]
async fn ai_commands_accessible_over_mcp_run() {
    let session = Session::default();
    let mcp = FilmcraftMcp::headless(session);

    // 1. ai.status via command_run
    let status_res = mcp.run("ai.status", json!({})).await.expect("ai.status over mcp");
    assert_eq!(status_res["configured"], json!(false));

    // 2. ai.setKey via command_run
    let set_key_res = mcp.run("ai.setKey", json!({ "key": "mcp_secret_key" })).await.expect("ai.setKey");
    assert_eq!(set_key_res["configured"], json!(true));

    let status_after = mcp.run("ai.status", json!({})).await.expect("ai.status after key");
    assert_eq!(status_after["configured"], json!(true));

    // 3. ai.characterAdd via command_run
    let add_char_res = mcp
        .run(
            "ai.characterAdd",
            json!({
                "id": "agent_smith",
                "name": "Smith",
                "description": "System sentinel"
            }),
        )
        .await
        .expect("characterAdd");
    assert_eq!(add_char_res["id"], json!("agent_smith"));

    // 4. ai.characterList
    let list_res = mcp.run("ai.characterList", json!({})).await.expect("characterList");
    let chars = list_res["characters"].as_array().expect("array of characters");
    assert_eq!(chars.len(), 1);
    assert_eq!(chars[0]["name"], json!("Smith"));

    // 5. ai.planScript with Fountain
    let plan_res = mcp
        .run(
            "ai.planScript",
            json!({
                "script": "EXT. CITY STREET - DAY\n\nSMITH\nMr. Anderson.\n",
                "title": "Encounter",
                "mode": "fountain"
            }),
        )
        .await
        .expect("planScript");
    assert_eq!(plan_res["title"], json!("Encounter"));

    // 6. ai.buildSequence
    let build_res = mcp
        .run(
            "ai.buildSequence",
            json!({
                "plan": plan_res,
                "name": "Encounter Timeline"
            }),
        )
        .await
        .expect("buildSequence");
    assert_eq!(build_res["status"], json!("ok"));
    assert_eq!(build_res["shot_count"], json!(1));
}
