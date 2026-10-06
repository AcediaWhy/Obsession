export const helixVertex = /* glsl */ `
attribute float aSlot;
uniform float uTime, uOpen, uClose, uConfirm, uRestore, uCandidateAge;
uniform vec2 uPointer;
uniform float uPointerPower;
varying vec3 vNormal, vWorld;
varying float vSlot, vWake;
mat2 turn(float a) { float s=sin(a), c=cos(a); return mat2(c,-s,s,c); }
void main() {
  float slot = aSlot;
  float opening = smoothstep(slot*.28, .85+slot*.28, uOpen);
  float closing = smoothstep(slot*.16, .72+slot*.16, uClose);
  float envelope = opening*(1.-closing);
  float front = uCandidateAge*.72-.2;
  float wave = exp(-pow((slot-front)*7.,2.));
  float phase = slot*10.8 + uTime*mix(.24,.09,uConfirm);
  phase += wave*.48;
  float spread = mix(1., .88, uConfirm)*mix(1., .72, uRestore);
  vec3 center = vec3((slot-.5)*7.05*mix(.12,1.,envelope), sin(phase)*.19, cos(phase)*.22);
  center.y += sin(slot*6.283+uTime*.35)*.07;
  float distanceToMouse = length(center.xy-uPointer);
  float touch = exp(-distanceToMouse*distanceToMouse*1.6)*uPointerPower;
  vec3 p = position;
  p.y *= spread*mix(.035,1.,envelope)*(1.+touch*.13+wave*.09);
  p.z *= mix(.3,1.,envelope);
  float flex = touch*.3*sin(uTime*3.-distanceToMouse*3.);
  p.yz = turn(phase+flex)*p.yz;
  p.xz = turn(touch*.14+wave*.08)*p.xz;
  center.xy += (uPointer-center.xy)*touch*.045;
  p += center;
  vec3 n=normal;
  n.yz = turn(phase+flex)*n.yz;
  n.xz = turn(touch*.14+wave*.08)*n.xz;
  // Oblique view exposes the depth of each plate and the turns of the helix.
  p.xz = turn(.5)*p.xz; p.xy = turn(-.13)*p.xy;
  n.xz = turn(.5)*n.xz; n.xy = turn(-.13)*n.xy;
  vNormal=normalize(normalMatrix*n);
  vWorld=p;
  vSlot=slot;
  vWake=touch+wave*.35;
  gl_Position=projectionMatrix*modelViewMatrix*vec4(p,1.);
}`;

export const helixFragment = /* glsl */ `
uniform float uTime, uConfirm, uRestore, uSuccess, uFinishAge;
uniform vec3 uAccent;
uniform vec4 uTrail[8];
varying vec3 vNormal, vWorld;
varying float vSlot, vWake;
vec3 spectrum(float x) { return .58+.42*cos(6.283*(x+vec3(.05,.36,.62))); }
void main() {
  vec3 n=normalize(vNormal);
  float fresnel=pow(1.-abs(n.z),2.4);
  float light=max(0.,dot(n,normalize(vec3(-.6,1.,1.4))));
  float highlight=pow(max(0.,dot(reflect(-normalize(vec3(-.8,1.3,2.)),n),vec3(0.,0.,1.))),22.);
  float gloss=pow(max(0.,dot(n,normalize(vec3(.25,.9,1.)))),9.);
  float wake=0.;
  for(int i=0;i<8;i++) {
    vec4 trail=uTrail[i];
    float age=max(0.,uTime-trail.w);
    vec2 d=vWorld.xy-trail.xy;
    wake += exp(-dot(d,d)*5.)*exp(-age*1.6)*trail.z;
  }
  wake=clamp(wake+vWake*.45,0.,1.);
  float pulse=exp(-pow((fract(uTime*mix(.16,.095,uConfirm)-vSlot*.86)-.5)*12.,2.));
  float echo=exp(-pow((fract(uTime*.095+vSlot*.86)-.5)*15.,2.))*uConfirm;
  float success=exp(-pow((vSlot-uFinishAge*1.75+.1)*8.,2.))*uSuccess;
  vec3 tint=mix(spectrum(vSlot*.66+n.y*.13+uTime*.025),uAccent,.48);
  vec3 frost=mix(vec3(.06,.08,.12),vec3(.28,.33,.43),light*.7);
  vec2 refracted=vWorld.xy+n.xy*.16;
  float inside=exp(-abs(refracted.y-sin(refracted.x*1.25-uTime*.35)*.28)*12.);
  frost += tint*inside*.10;
  vec3 glass=mix(frost,tint,wake*.6+fresnel*.2);
  glass += vec3(.5,.56,.7)*gloss*.22;
  glass += mix(vec3(.72,.78,.95),tint,.4)*(fresnel*.38+highlight*.72);
  glass += tint*(pulse*.2+echo*.13)*(1.-uRestore*.7);
  glass += mix(vec3(.6,1.,.8),uAccent,.25)*success*.7;
  glass *= mix(1.,.65,uRestore);
  gl_FragColor=vec4(glass,1.);
  #include <colorspace_fragment>
}`;

export const fieldVertex = /* glsl */ `
varying vec2 vPosition;
void main(){ vPosition=position.xy; gl_Position=projectionMatrix*modelViewMatrix*vec4(position,1.); }
`;

export const fieldFragment = /* glsl */ `
uniform float uTime, uOpen, uClose, uRestore, uConfirm;
uniform vec3 uAccent;
uniform vec2 uPointer;
uniform float uPointerPower;
varying vec2 vPosition;
void main(){
  vec2 p=vPosition;
  float bend=exp(-pow((p.x-uPointer.x)*.8,2.))*uPointer.y*uPointerPower*.12;
  float lineA=sin(p.x*1.25-uTime*.35)*.28+bend;
  float lineB=sin(p.x*1.25-uTime*.35+2.)*.28+bend;
  float a=abs(p.y-lineA), b=abs(p.y-lineB);
  float lines=exp(-a*100.)*.5+exp(-b*85.)*.22;
  float haze=exp(-a*9.)*.1+exp(-b*7.)*.065;
  float pulse=pow(max(0.,sin(p.x*1.4-uTime*1.1)),12.);
  float edge=1.-smoothstep(3.,4.35,abs(p.x));
  float envelope=smoothstep(0.,1.,uOpen)*(1.-smoothstep(0.,.9,uClose));
  vec3 color=mix(uAccent,vec3(.45,.63,.95),.35);
  float alpha=(lines*(.3+pulse*.7)+haze)*edge*envelope*(1.-uRestore*.6);
  gl_FragColor=vec4(color,alpha);
  #include <colorspace_fragment>
}`;
