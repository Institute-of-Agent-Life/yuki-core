"""A deterministic refusal retains its request ID even after the precondition changes."""
import json
import os
import uuid
from tests.harness.compact import Owner


def run(ctx,wire):
    owner=Owner(ctx,'control-refusal',40)
    token='fixture-failed-command-0000000000000000000000'
    environment={**os.environ,'CONTROL_REFUSAL':token}
    registry=owner.svc.data/'operators.toml'
    original='[[operators]]\nprincipal_id='+json.dumps(str(uuid.uuid4()))+'\ntoken_env="CONTROL_REFUSAL"\ncapabilities=["control.character.read","control.character.mutate","control.operation.read"]\n'
    registry.write_text(original)
    cfg=owner.svc.data/'yuki.toml';cfg.write_text('operator_registry_file="operators.toml"\n'+cfg.read_text())
    def patch(body):return wire(owner.svc.address,'/admin/v1/character',token,body,'PATCH')
    def receipt(body):return wire(owner.svc.address,'/admin/v1/operations/'+body['request_id'],token)[1]['data']
    with owner.svc.running(environment=environment):
        current=owner.op('control','read','character.get')
        failed={'request_id':str(uuid.uuid4()),'expected_revision':current['revision']+1,'display_name':'original refused request'}
        ctx.equal('future revision initially conflicts',patch(failed)[0],409)
        saved=receipt(failed)
        ctx.equal('deterministic failure has a readable original receipt',[saved['status'],saved['error_category']],['failed','version_conflict'])
        success={**failed,'request_id':str(uuid.uuid4()),'expected_revision':current['revision'],'display_name':'accepted new request'}
        ctx.equal('independent valid write reaches refused target revision',patch(success)[0],200)
        ctx.equal('same failed request never becomes a fresh write',patch(failed)[0],409)
        alias=patch({**failed,'request_id':failed['request_id'].upper()})
        ctx.equal('UUID casing preserves the same failed request',alias[1]['problem']['code'],'version_conflict')
        ctx.equal('UUID casing preserves the same successful request',patch({**success,'request_id':success['request_id'].upper()})[0],200)
        ctx.equal('cached failure preserves original timestamps and metadata',receipt(failed),saved)
        ctx.equal('failed replay does not change the character',owner.op('control','read','character.get')['display_name'],'accepted new request')
        conflict=patch({**failed,'display_name':'changed payload'})
        ctx.equal('failed ID also prevents changed payload',conflict[1]['problem']['code'],'request_conflict')
        registry.write_text(original.replace('control.character.mutate','control.preference.read'))
        ctx.equal('current revocation wins before reading cached failure',patch(failed)[0],403)
        registry.write_text(original)
        invalid={**failed,'request_id':str(uuid.uuid4()),'display_name':42,'unexpected':'private rejected body'}
        ctx.equal('invalid mutation body is rejected',patch(invalid)[0],422)
        ctx.equal('malformed DTO is refused before command admission',wire(owner.svc.address,'/admin/v1/operations/'+invalid['request_id'],token)[0],404)
        repaired={key:value for key,value in invalid.items() if key!='unexpected'}
        repaired['display_name']='repaired malformed DTO'
        ctx.equal('unadmitted malformed DTO has not consumed the request ID',patch(repaired)[0],200)
        ctx.check('failure metadata never includes the rejected body','private rejected body' not in json.dumps(receipt(invalid)))
    with owner.svc.running(environment=environment):
        ctx.equal('cold refusal replay remains the same failure',patch(failed)[0],409)
        ctx.equal('cold receipt preserves original failure',receipt(failed),saved)
    return {'failed_receipt':saved,'cold_replay':'same original refusal','partial_mutation_rollback':'Rust single-writer regression'}
