import assert from "node:assert/strict";
import {accountInput, type DirectAccountInput} from "./directConnection";

const account: DirectAccountInput = {id: "saved", label: " Account ", region: "cn", login_method: "password",
    username: " name ", uid: "", oauth_type: 21, client_version_string: "", route: null};
assert.deepEqual(accountInput({...account, password: "", code: ""}), {...account, label: "Account", username: "name"});
assert.equal(accountInput({...account, password: " password "}).password, " password ");
assert.equal("code" in accountInput({...account, code: "unrelated-secret"}), false);
const oauth = accountInput({...account, login_method: "oauth", uid: " 123 ", password: "unrelated-secret", code: " code "});
assert.equal("password" in oauth, false);
assert.equal(oauth.code, "code");
assert.equal(oauth.uid, "123");
assert.equal("code" in accountInput({...account, login_method: "oauth", code: "  "}), false);
const international = accountInput({...account, region: "en", login_method: "oauth", oauth_type: 7, client_version_string: "WebGL_2022-1.2.3"});
assert.equal(international.oauth_type, 7);
assert.equal(international.client_version_string, "WebGL_2022-1.2.3");
const email = accountInput({...account, region: "jp", login_method: "email", verification_code: " 123456 ", password: "unused", code: "unused"});
assert.equal(email.verification_code, "123456");
assert.equal("code" in email, false);
assert.equal("password" in email, false);
assert.equal("verification_code" in accountInput({...account, verification_code: "123456"}), false);
