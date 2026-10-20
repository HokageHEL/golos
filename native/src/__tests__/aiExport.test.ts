import { expect, it } from 'vitest';
import { exportAiResult } from '../export/ai';
import type { AiWork } from '../api';
const work:AiWork={id:'internal',noteId:'private-path-id',kind:'summary',target:'',prompt:'private request',conversation:'session',sourceLanguage:'auto',model:'local',contextWindow:8192,source:[],sourceHash:'private hash',status:'done',content:'Answer <script> [s1]',segments:[],createdAt:123,error:null,phase:'Complete'};
it('free-form JSON has a stable user-facing schema without transcript snapshots or internal IDs',()=>{
  const json=JSON.parse(exportAiResult('Summary',work,'json'));
  expect(json).toEqual({version:1,title:'Summary',kind:'summary',targetLanguage:null,model:'local',createdAt:123,content:'Answer <script> [s1]'});
  expect(exportAiResult('Summary',work,'txt')).toBe('Answer <script> [s1]\n');
  expect(()=>exportAiResult('Summary',work,'srt')).toThrow('Untimed');
});
it('structured translation export retains native times, speakers and Unicode',()=>{
  const translated={...work,kind:'translate',segments:[{startMs:1234,endMs:2500,speaker:'Alice',text:'Привіт 🌍'}]};
  const vtt=exportAiResult('Переклад',translated,'vtt');expect(vtt).toContain('00:00:01.234 --> 00:00:02.500');expect(vtt).toContain('Привіт 🌍');expect(vtt).toContain('Alice');
});
