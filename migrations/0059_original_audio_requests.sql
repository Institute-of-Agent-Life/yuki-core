-- Reader 59. Retain the original accounting key after the attachment is erased.
CREATE TABLE host_aux_transcripts(
 plugin TEXT NOT NULL REFERENCES host_installations(plugin),
 source INTEGER NOT NULL REFERENCES events(id),
 segment INTEGER NOT NULL CHECK(segment>=0),
 request TEXT NOT NULL UNIQUE REFERENCES host_aux_requests(id),
 PRIMARY KEY(plugin,source,segment));

-- Multiple legacy reservations remain in the ledger. Preserve the first one
-- for future lookups, including an original attempt whose outcome is unknown.
INSERT OR IGNORE INTO host_aux_transcripts
 SELECT q.plugin,s.source,s.segment,q.id
 FROM host_aux_requests q
 JOIN host_calls h ON h.handle=json_extract(q.origin,'$.handle') AND h.plugin=q.plugin
 JOIN host_media_selected s ON s.handle=h.handle
 JOIN model_requests r ON r.id=q.logical
 WHERE h.method='media.process' AND q.purpose='transcript' AND s.purpose=q.purpose
   AND s.source IS NOT NULL AND s.segment IS NOT NULL
   AND (json_type(r.request_json,'$.messages[#-1].content[0].input_audio.data')='text'
        OR json_extract(r.request_json,'$.contents[#-1].parts[0].inlineData.mimeType') LIKE 'audio/%')
 ORDER BY q.rowid;
