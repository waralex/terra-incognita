import test from 'node:test';
import assert from 'node:assert/strict';
import {localViewerRequest} from './document-viewer-access.mjs';
test('local viewer rejects rebinding and cross-origin browser access',()=>{
 assert.ok(localViewerRequest({host:'127.0.0.1:8096'}));
 assert.ok(localViewerRequest({host:'localhost:8096',origin:'http://localhost:8096'}));
 for(const headers of [{host:'attacker.example:8096'},{host:'localhost.evil'}, {host:'localhost:8096',origin:'https://attacker.example'}, {host:'localhost:8096',origin:'null'}, {host:'localhost:8096','sec-fetch-site':'cross-site'}, {}])assert.equal(localViewerRequest(headers),false);
});
