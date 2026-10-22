# 工具来源审计

官方文档：`@snowluma/mcp` 2.1.1；抓取时间 `2026-10-22T09:25:29.990Z`。
目录 hash：`0d7b4a9d6d173363ea139f09dcc67216fa912564662d4f1f6601449c66bc65db`。

检查通过：200 个官方工具、443 个官方参数，含 1 个官方 MCP 本地主机执行选项。

## 来源规则

- 所有官方工具名称及 action 参数字段 schema 与快照完全一致；描述中的官方摘要、约束和别名保留原文。
- 缺失摘要时复用已有官方返回描述；仍缺失时将补充文本标注为自写。自写文本仅记录来源，不据此判断其业务语义。
- 官方返回 schema 原样约束成功响应的 data 字段；外层条件封套由本项目定义，失败响应保留完整 OneBot 数据。
- upload_file_stream 增加官方 invoke_action 的 execution 选项；stream_id 在自动上传时由 MCP 生成，原始字段仍全部保留，必填条件调整为手动分块或自动上传二选一。
- JSON 报告包含完整工具声明、参数 schema、描述原文及来源 JSON Pointer。

## 自写描述

| 工具 | 描述 | 来源 |
| --- | --- | --- |
| `cancel_group_album_media_like` | 取消群相册媒体点赞 | authored:src/patches.rs |
| `do_group_album_comment` | 评论群相册媒体 | authored:src/patches.rs |
| `set_group_album_media_like` | 点赞群相册媒体 | authored:src/patches.rs |
| `upload_image_to_qun_album` | 上传图片到群相册 | authored:src/patches.rs |

## MCP 执行选项

`upload_file_stream.execution` 和 `execution.input_file` 原文来自官方 MCP。

手动分块需要 stream_id；自动上传需要 execution.input_file。此条件适配由本项目定义，不改变任何官方 action 参数字段。


## 服务器说明

来源：`authored:src/server.rs`。

SnowLuma QQ API。先检查 isError，再读 structuredContent；普通响应还需检查 status/retcode。接口行为可能受 QQ 客户端限制；异常请告知用户。

## 逐工具清单

参数 ? 表示非无条件必填；upload_file_stream 的 stream_id/execution 存在上述条件要求。完整描述与逐参数来源见 [tool-audit.json](tool-audit.json)。

| 工具 | 参数 | 描述来源 |
| --- | --- | --- |
| `.get_word_slices` | — | assets/catalog.json#/actions/0/summary |
| `_del_group_notice` | `fid?`, `group_id`, `notice_id?` | assets/catalog.json#/actions/1/summary |
| `_get_friend_dress` | `user_id` | assets/catalog.json#/actions/2/summary |
| `_get_group_notice` | `group_id` | assets/catalog.json#/actions/3/summary |
| `_get_model_show` | `model?` | assets/catalog.json#/actions/4/summary |
| `_mark_all_as_read` | — | assets/catalog.json#/actions/5/summary |
| `_send_group_notice` | `confirm_required?`, `content`, `group_id`, `image?`, `is_show_edit_card?`, `pinned?`, `send_to_new_members?`, `tip_window_type?`, `type?` | assets/catalog.json#/actions/6/summary<br>assets/catalog.json#/actions/6/invariants/0<br>assets/catalog.json#/actions/6/invariants/1 |
| `_set_model_show` | — | assets/catalog.json#/actions/7/summary |
| `add_custom_face` | `file` | assets/catalog.json#/actions/8/summary |
| `add_group_tag` | `group_class`, `name` | assets/catalog.json#/actions/9/summary |
| `bot_exit` | — | assets/catalog.json#/actions/10/summary |
| `can_send_image` | — | assets/catalog.json#/actions/11/returnsSchema/properties/yes/description |
| `can_send_record` | — | assets/catalog.json#/actions/12/returnsSchema/properties/yes/description |
| `cancel_group_album_media_like` | `album_id`, `batch_id`, `group_id`, `lloc?` | authored:src/patches.rs |
| `cancel_group_todo` | `group_id`, `message_id` | assets/catalog.json#/actions/14/summary |
| `check_url_safely` | — | assets/catalog.json#/actions/15/summary |
| `clean_cache` | — | assets/catalog.json#/actions/16/summary |
| `clean_stream_temp_file` | — | assets/catalog.json#/actions/17/summary |
| `click_inline_keyboard_button` | `bot_appid`, `button_id`, `callback_data?`, `group_id`, `msg_seq` | assets/catalog.json#/actions/18/summary |
| `comment_qzone` | `content`, `images?`, `target_uin?`, `tid` | assets/catalog.json#/actions/19/summary |
| `complete_group_todo` | `group_id`, `message_id` | assets/catalog.json#/actions/20/summary |
| `create_collection` | — | assets/catalog.json#/actions/21/summary |
| `create_flash_task` | `files?`, `name?`, `thumb_path?` | assets/catalog.json#/actions/22/summary |
| `create_group_file_folder` | `group_id`, `name`, `parent_id?` | assets/catalog.json#/actions/23/summary |
| `del_group_album_media` | `album_id`, `group_id`, `lloc` | assets/catalog.json#/actions/24/summary |
| `delete_custom_face` | `emoji_id` | assets/catalog.json#/actions/25/summary |
| `delete_essence_msg` | `message_id` | assets/catalog.json#/actions/26/summary |
| `delete_flash_file` | `fileset_id` | assets/catalog.json#/actions/27/summary |
| `delete_friend` | `block?`, `user_id` | assets/catalog.json#/actions/28/summary |
| `delete_group_file` | `file_id`, `group_id` | assets/catalog.json#/actions/29/summary |
| `delete_group_file_folder` | `folder_id`, `group_id` | assets/catalog.json#/actions/30/summary |
| `delete_group_folder` | `folder_id`, `group_id` | assets/catalog.json#/actions/31/summary |
| `delete_group_tag` | `group_class`, `name` | assets/catalog.json#/actions/32/summary |
| `delete_msg` | `message_id` | assets/catalog.json#/actions/33/summary |
| `delete_qzone_msg` | `tid` | assets/catalog.json#/actions/34/summary |
| `do_group_album_comment` | `album_id`, `content`, `group_id`, `lloc` | authored:src/patches.rs |
| `download_file` | `base64?`, `headers?`, `name?`, `url?` | assets/catalog.json#/actions/36/summary |
| `download_file_image_stream` | `chunk_size?`, `file?`, `file_id?` | assets/catalog.json#/actions/37/summary |
| `download_file_record_stream` | `chunk_size?`, `file?`, `file_id?` | assets/catalog.json#/actions/38/summary |
| `download_file_stream` | `chunk_size?`, `file?`, `file_id?` | assets/catalog.json#/actions/39/summary |
| `download_fileset` | `file_index?`, `file_name?`, `fileset_id` | assets/catalog.json#/actions/40/summary |
| `fetch_custom_face` | `count?`, `return_type?` | assets/catalog.json#/actions/41/summary |
| `fetch_custom_face_detail` | `count?` | assets/catalog.json#/actions/42/summary |
| `fetch_emoji_like` | `cookie?`, `count?`, `emojiId`, `message_id` | assets/catalog.json#/actions/43/summary |
| `fetch_face_entity` | `face_id`, `refresh?` | assets/catalog.json#/actions/44/summary |
| `fetch_ptt_text` | `message_id` | assets/catalog.json#/actions/45/summary<br>assets/catalog.json#/actions/45/aliases |
| `fetch_super_face_id` | `face_id`, `refresh?` | assets/catalog.json#/actions/46/summary |
| `fetch_sys_faces` | `refresh?` | assets/catalog.json#/actions/47/summary |
| `forward_friend_single_msg` | `message_id`, `user_id` | assets/catalog.json#/actions/48/summary |
| `forward_group_single_msg` | `group_id`, `message_id` | assets/catalog.json#/actions/49/summary |
| `friend_poke` | `target_id?`, `user_id` | assets/catalog.json#/actions/50/summary |
| `get_ai_characters` | `chat_type?`, `group_id` | assets/catalog.json#/actions/51/summary |
| `get_ai_record` | `character`, `chat_type?`, `group_id`, `text` | assets/catalog.json#/actions/52/summary |
| `get_clientkey` | — | assets/catalog.json#/actions/53/summary |
| `get_collection_list` | `category?`, `count?` | assets/catalog.json#/actions/54/summary |
| `get_cookies` | `domain?` | assets/catalog.json#/actions/55/summary |
| `get_credentials` | `domain?` | assets/catalog.json#/actions/56/summary |
| `get_csrf_token` | — | assets/catalog.json#/actions/57/summary |
| `get_doubt_friends_add_request` | `count?` | assets/catalog.json#/actions/58/summary |
| `get_emoji_likes` | `emoji_id`, `message_id` | assets/catalog.json#/actions/59/summary |
| `get_essence_msg_list` | `group_id` | assets/catalog.json#/actions/60/summary |
| `get_file` | `file?`, `file_id?` | assets/catalog.json#/actions/61/summary |
| `get_fileset_id` | `share_code` | assets/catalog.json#/actions/62/summary |
| `get_fileset_info` | `fileset_id` | assets/catalog.json#/actions/63/summary |
| `get_flash_file_list` | `fileset_id` | assets/catalog.json#/actions/64/summary |
| `get_flash_file_url` | `file_index?`, `file_name?`, `fileset_id` | assets/catalog.json#/actions/65/summary |
| `get_forward_msg` | `id?`, `message_id?` | assets/catalog.json#/actions/66/summary |
| `get_friend_list` | — | assets/catalog.json#/actions/67/summary |
| `get_friend_msg_history` | `count?`, `message_id?`, `reverse_order?`, `user_id` | assets/catalog.json#/actions/68/summary |
| `get_friends_with_category` | — | assets/catalog.json#/actions/69/summary |
| `get_group_admin_settings` | `group_id` | assets/catalog.json#/actions/70/summary |
| `get_group_album_list` | `group_id` | assets/catalog.json#/actions/71/returnsSchema/description |
| `get_group_album_media_list` | `album_id`, `attach_info?`, `group_id` | assets/catalog.json#/actions/72/returns |
| `get_group_at_all_remain` | `group_id` | assets/catalog.json#/actions/73/summary |
| `get_group_detail_info` | `group_id`, `no_cache?` | assets/catalog.json#/actions/74/summary |
| `get_group_file_system_info` | `group_id` | assets/catalog.json#/actions/75/summary |
| `get_group_file_url` | `busid?`, `file_id`, `group_id` | assets/catalog.json#/actions/76/summary |
| `get_group_files_by_folder` | `folder?`, `folder_id?`, `group_id` | assets/catalog.json#/actions/77/summary |
| `get_group_honor_info` | `group_id`, `type?` | assets/catalog.json#/actions/78/summary |
| `get_group_ignore_add_request` | — | assets/catalog.json#/actions/79/summary |
| `get_group_ignored_notifies` | — | assets/catalog.json#/actions/80/summary |
| `get_group_info` | `group_id`, `no_cache?` | assets/catalog.json#/actions/81/summary |
| `get_group_info_ex` | `group_id`, `no_cache?` | assets/catalog.json#/actions/82/summary |
| `get_group_list` | `no_cache?` | assets/catalog.json#/actions/83/summary |
| `get_group_member_info` | `group_id`, `no_cache?`, `user_id` | assets/catalog.json#/actions/84/summary |
| `get_group_member_list` | `group_id`, `no_cache?` | assets/catalog.json#/actions/85/summary |
| `get_group_msg_history` | `count?`, `group_id`, `message_id?`, `reverse_order?` | assets/catalog.json#/actions/86/summary |
| `get_group_root_files` | `group_id` | assets/catalog.json#/actions/87/summary |
| `get_group_share_link` | `additional_param?`, `group_id`, `need_short_url?`, `src_id?` | assets/catalog.json#/actions/88/summary |
| `get_group_shut_list` | `group_id` | assets/catalog.json#/actions/89/summary |
| `get_group_signed_list` | `group_id` | assets/catalog.json#/actions/90/summary |
| `get_group_system_msg` | `count?`, `group_id?`, `only_pending?` | assets/catalog.json#/actions/91/summary |
| `get_group_tags` | `group_class` | assets/catalog.json#/actions/92/summary |
| `get_group_todo_list` | `group_id` | assets/catalog.json#/actions/93/summary |
| `get_image` | `file?`, `file_id?` | assets/catalog.json#/actions/94/summary |
| `get_login_info` | — | assets/catalog.json#/actions/95/returns |
| `get_mini_app_ark` | `desc?`, `jumpUrl?`, `jump_url?`, `picUrl?`, `pic_url?`, `title?`, `type?` | assets/catalog.json#/actions/96/summary |
| `get_msg` | `message_id` | assets/catalog.json#/actions/97/summary |
| `get_msg_emoji_likes` | `message_id` | assets/catalog.json#/actions/98/summary |
| `get_online_clients` | `no_cache?` | assets/catalog.json#/actions/99/summary |
| `get_private_file_url` | `file_hash?`, `file_id`, `user_id?` | assets/catalog.json#/actions/100/summary |
| `get_profile_like` | `count?`, `start?`, `user_id?` | assets/catalog.json#/actions/101/summary |
| `get_qun_album_list` | `attach_info?`, `group_id` | assets/catalog.json#/actions/102/returns |
| `get_qzone_feeds` | `count?`, `page_num?` | assets/catalog.json#/actions/103/summary |
| `get_qzone_msg_detail` | `num?`, `pos?`, `target_uin?`, `tid` | assets/catalog.json#/actions/104/summary |
| `get_qzone_msg_list` | `num?`, `pos?`, `target_uin?` | assets/catalog.json#/actions/105/summary |
| `get_recent_contact` | `count?` | assets/catalog.json#/actions/106/summary |
| `get_record` | `file?`, `file_id?`, `out_format?` | assets/catalog.json#/actions/107/summary |
| `get_rkey` | — | assets/catalog.json#/actions/108/summary<br>assets/catalog.json#/actions/108/aliases |
| `get_rkey_server` | — | assets/catalog.json#/actions/109/summary |
| `get_share_link` | `fileset_id` | assets/catalog.json#/actions/110/summary |
| `get_status` | — | assets/catalog.json#/actions/111/returns |
| `get_stranger_info` | `user_id` | assets/catalog.json#/actions/112/summary |
| `get_unidirectional_friend_list` | — | assets/catalog.json#/actions/113/summary |
| `get_version_info` | — | assets/catalog.json#/actions/114/returns |
| `group_poke` | `group_id`, `user_id` | assets/catalog.json#/actions/115/summary |
| `like_qzone` | `abstime?`, `target_uin?`, `tid` | assets/catalog.json#/actions/116/summary |
| `list_filesets` | — | assets/catalog.json#/actions/117/summary |
| `mark_group_msg_as_read` | `group_id?`, `message_id` | assets/catalog.json#/actions/118/summary |
| `mark_msg_as_read` | `message_id`, `target_id?` | assets/catalog.json#/actions/119/summary |
| `mark_private_msg_as_read` | `message_id`, `user_id?` | assets/catalog.json#/actions/120/summary |
| `modify_custom_face` | `desc?`, `emoji_id` | assets/catalog.json#/actions/121/summary |
| `move_custom_face_to_front` | `emoji_id` | assets/catalog.json#/actions/122/summary |
| `move_group_file` | `file_id`, `group_id`, `parent_directory`, `target_directory` | assets/catalog.json#/actions/123/summary |
| `nc_get_packet_status` | — | assets/catalog.json#/actions/124/summary |
| `nc_get_user_status` | `user_id` | assets/catalog.json#/actions/125/summary |
| `ocr_image` | `image` | assets/catalog.json#/actions/126/summary<br>assets/catalog.json#/actions/126/aliases |
| `rename_flash_file` | `fileset_id`, `new_name` | assets/catalog.json#/actions/127/summary |
| `rename_group_file` | `current_parent_directory?`, `file_id`, `group_id`, `new_name` | assets/catalog.json#/actions/128/summary |
| `rename_group_file_folder` | `folder_id`, `group_id`, `name?`, `new_folder_name?` | assets/catalog.json#/actions/129/summary |
| `request_decrypt_key` | `db_path` | assets/catalog.json#/actions/130/summary |
| `search_sys_faces` | `query` | assets/catalog.json#/actions/131/summary |
| `send_ark_share` | `group_id?`, `phone_number?`, `user_id?` | assets/catalog.json#/actions/132/summary |
| `send_custom_face` | `emoji_id`, `group_id?`, `reply_to?`, `user_id?` | assets/catalog.json#/actions/133/summary |
| `send_flash_msg` | `fileset_id`, `group_id?`, `user_id?` | assets/catalog.json#/actions/134/summary |
| `send_forward_msg` | `group_id?`, `message?`, `message_type?`, `messages?`, `news?`, `prompt?`, `source?`, `summary?`, `user_id?` | assets/catalog.json#/actions/135/summary |
| `send_group_ai_record` | `character`, `chat_type?`, `group_id`, `text` | assets/catalog.json#/actions/136/summary |
| `send_group_ark_share` | `group_id` | assets/catalog.json#/actions/137/summary |
| `send_group_forward_msg` | `group_id`, `message?`, `messages?`, `news?`, `prompt?`, `source?`, `summary?` | assets/catalog.json#/actions/138/summary |
| `send_group_msg` | `auto_escape?`, `group_id`, `message` | assets/catalog.json#/actions/139/summary |
| `send_like` | `times?`, `user_id` | assets/catalog.json#/actions/140/summary |
| `send_msg` | `auto_escape?`, `group_id?`, `message`, `message_type?`, `user_id?` | assets/catalog.json#/actions/141/summary |
| `send_packet` | `cmd`, `data?`, `rsp?` | assets/catalog.json#/actions/142/summary<br>assets/catalog.json#/actions/142/aliases |
| `send_poke` | `group_id?`, `user_id` | assets/catalog.json#/actions/143/summary |
| `send_private_forward_msg` | `message?`, `messages?`, `news?`, `prompt?`, `source?`, `summary?`, `user_id` | assets/catalog.json#/actions/144/summary |
| `send_private_msg` | `auto_escape?`, `group_id?`, `message`, `user_id` | assets/catalog.json#/actions/145/summary |
| `send_qzone_msg` | `content`, `images?`, `target_uins?`, `ugc_right?` | assets/catalog.json#/actions/146/summary<br>assets/catalog.json#/actions/146/invariants/0<br>assets/catalog.json#/actions/146/invariants/1 |
| `send_tuwen_ark` | `desc`, `group_id?`, `jump_url`, `preview_url?`, `summary?`, `title`, `user_id?` | assets/catalog.json#/actions/147/summary |
| `set_diy_online_status` | `face_id`, `face_type?`, `wording?` | assets/catalog.json#/actions/148/summary |
| `set_doubt_friends_add_request` | `approve?`, `flag` | assets/catalog.json#/actions/149/summary |
| `set_essence_msg` | `message_id` | assets/catalog.json#/actions/150/summary |
| `set_friend_add_request` | `approve?`, `flag` | assets/catalog.json#/actions/151/summary |
| `set_friend_remark` | `remark`, `user_id` | assets/catalog.json#/actions/152/summary |
| `set_friends_category` | `categoryId?`, `categoryName?`, `uin` | assets/catalog.json#/actions/153/summary<br>assets/catalog.json#/actions/153/invariants/0 |
| `set_group_add_option` | `add_type?`, `group_answer?`, `group_id`, `group_question?` | assets/catalog.json#/actions/154/summary |
| `set_group_add_request` | `approve?`, `flag`, `reason?`, `sub_type?`, `type?` | assets/catalog.json#/actions/155/summary |
| `set_group_admin` | `enable?`, `group_id`, `user_id` | assets/catalog.json#/actions/156/summary |
| `set_group_album_media_like` | `album_id`, `batch_id`, `group_id`, `lloc?` | authored:src/patches.rs |
| `set_group_anonymous` | — | assets/catalog.json#/actions/158/summary |
| `set_group_anonymous_ban` | — | assets/catalog.json#/actions/159/summary |
| `set_group_ban` | `duration?`, `group_id`, `user_id` | assets/catalog.json#/actions/160/summary |
| `set_group_card` | `card?`, `group_id`, `user_id` | assets/catalog.json#/actions/161/summary |
| `set_group_kick` | `group_id`, `reject_add_request?`, `user_id` | assets/catalog.json#/actions/162/summary |
| `set_group_kick_members` | `group_id`, `reject_add_request?`, `user_id` | assets/catalog.json#/actions/163/summary |
| `set_group_leave` | `group_id` | assets/catalog.json#/actions/164/summary |
| `set_group_member_invite_policy` | `group_id`, `policy` | assets/catalog.json#/actions/165/summary |
| `set_group_member_permissions` | `allow_member_create_group?`, `allow_member_temporary_session?`, `allow_member_upload_album?`, `group_id` | assets/catalog.json#/actions/166/summary<br>assets/catalog.json#/actions/166/invariants/0 |
| `set_group_name` | `group_id`, `group_name?` | assets/catalog.json#/actions/167/summary |
| `set_group_new_member_history_visibility` | `group_id`, `visible` | assets/catalog.json#/actions/168/summary |
| `set_group_portrait` | `file`, `group_id` | assets/catalog.json#/actions/169/summary |
| `set_group_reaction` | `code`, `group_id?`, `is_set?`, `message_id` | assets/catalog.json#/actions/170/summary |
| `set_group_remark` | `group_id`, `remark` | assets/catalog.json#/actions/171/summary |
| `set_group_robot_add_option` | `group_id`, `robot_member_examine?`, `robot_member_switch?` | assets/catalog.json#/actions/172/summary |
| `set_group_search` | `group_id`, `no_code_finger_open?`, `no_finger_open?` | assets/catalog.json#/actions/173/summary |
| `set_group_sign` | `group_id` | assets/catalog.json#/actions/174/summary<br>assets/catalog.json#/actions/174/aliases |
| `set_group_special_title` | `group_id`, `special_title?`, `user_id` | assets/catalog.json#/actions/175/summary |
| `set_group_todo` | `group_id`, `message_id` | assets/catalog.json#/actions/176/summary |
| `set_group_whole_ban` | `enable?`, `group_id` | assets/catalog.json#/actions/177/summary |
| `set_input_status` | `event_type?`, `user_id` | assets/catalog.json#/actions/178/summary |
| `set_msg_emoji_like` | `emoji_id`, `message_id`, `set?` | assets/catalog.json#/actions/179/summary |
| `set_online_status` | `battery_status?`, `ext_status?`, `status` | assets/catalog.json#/actions/180/summary |
| `set_qq_avatar` | `file` | assets/catalog.json#/actions/181/summary |
| `set_qq_profile` | `nickname?`, `personal_note?`, `sex?` | assets/catalog.json#/actions/182/summary |
| `set_qzone_ban` | `enable?`, `user_id` | assets/catalog.json#/actions/183/summary |
| `set_qzone_msg_right` | `target_uins?`, `tid`, `ugc_right` | assets/catalog.json#/actions/184/summary<br>assets/catalog.json#/actions/184/invariants/0<br>assets/catalog.json#/actions/184/invariants/1 |
| `set_restart` | — | assets/catalog.json#/actions/185/summary |
| `set_self_longnick` | `longNick?`, `long_nick?` | assets/catalog.json#/actions/186/summary |
| `share_group_ex` | `group_id` | assets/catalog.json#/actions/187/summary |
| `share_peer` | `group_id?`, `phone_number?`, `user_id?` | assets/catalog.json#/actions/188/summary |
| `test_download_stream` | `error?` | assets/catalog.json#/actions/189/summary |
| `trans_group_file` | `file_id`, `group_id` | assets/catalog.json#/actions/190/summary |
| `translate_en2zh` | `words` | assets/catalog.json#/actions/191/summary |
| `unlike_qzone` | `abstime?`, `target_uin?`, `tid` | assets/catalog.json#/actions/192/summary |
| `upload_file_stream` | `chunk_data?`, `chunk_index?`, `execution?`, `expected_sha256?`, `file_retention?`, `file_size?`, `filename?`, `is_complete?`, `reset?`, `stream_id?`, `total_chunks?`, `verify_only?` | assets/catalog.json#/actions/193/summary |
| `upload_forward_msg` | `group_id?`, `message?`, `messages?` | assets/catalog.json#/actions/194/summary |
| `upload_foward_msg` | `group_id?`, `message?`, `messages?` | assets/catalog.json#/actions/195/summary |
| `upload_group_file` | `file`, `folder?`, `folder_id?`, `group_id`, `name?`, `upload_file?` | assets/catalog.json#/actions/196/summary |
| `upload_image_to_qun_album` | `album_id`, `album_name`, `file`, `group_id` | authored:src/patches.rs |
| `upload_private_file` | `file`, `name?`, `upload_file?`, `user_id` | assets/catalog.json#/actions/198/summary |
| `upload_video_to_qun_album` | `album_id`, `album_name`, `file`, `group_id` | assets/catalog.json#/actions/199/summary |
