import { useState } from "react";
import type { Zapret2CatalogProfile } from "../../lib/tauri";
import { Button } from "./atoms";

export function Zapret2CompositionTrial({ profiles, disabled, test }: {
  profiles:Zapret2CatalogProfile[]; disabled:boolean;
  test:(pair:{apiProfileId:string;cdnProfileId:string})=>void;
}) {
  const [apiChoice,setApi]=useState("");
  const [cdnChoice,setCdn]=useState("");
  const parents=profiles.filter((profile)=>profile.compositionEligible);
  if(parents.length<2)return null;
  const api=parents.some((parent)=>parent.profileId===apiChoice) ? apiChoice : (parents.find((parent)=>parent.selected) ?? parents[0]).profileId;
  const cdn=parents.some((parent)=>parent.profileId===cdnChoice) ? cdnChoice : parents.find((parent)=>parent.profileId!==api)!.profileId;
  return <details className="text-xs text-ink-muted">
    <summary className="cursor-pointer text-ink-soft">Испытать своё сочетание</summary>
    <div className="mt-2 grid gap-2">
      <p>Выбери два профиля для отдельного испытания. Текущая конфигурация показана выше.</p>
      {([{label:"API / Gateway",value:api,set:setApi},{label:"CDN",value:cdn,set:setCdn}]).map((field)=><label key={field.label} className="grid gap-1">{field.label}
        <select aria-label={`Профиль сборки ${field.label}`} value={field.value} disabled={disabled}
          className="no-drag rounded-lg border px-2 py-2" onChange={(event)=>field.set(event.target.value)}>
          {parents.map((parent)=><option key={parent.profileId} value={parent.profileId}>{parent.name}</option>)}
        </select>
      </label>)}
      <p>Проверим сочетание целиком. Применить результат можно после проверки.</p>
      <Button variant="ghost" disabled={disabled || api===cdn} onClick={()=>test({apiProfileId:api,cdnProfileId:cdn})}>Испытать сочетание</Button>
    </div>
  </details>;
}
