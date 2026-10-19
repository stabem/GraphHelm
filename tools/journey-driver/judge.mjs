// Pure snapshot judgment. Browser observation and secret cleaning stay in the driver.
function distance(a, b) {
  let previous = Array.from({length:b.length+1},(_,i)=>i);
  for (let i=0;i<a.length;i++) {
    const next=[i+1];
    for (let j=0;j<b.length;j++) next.push(Math.min(next[j]+1,previous[j+1]+1,previous[j]+(a[i]===b[j]?0:1)));
    previous=next;
  }
  return previous[b.length];
}

// #1041: an `absent` expectation is met when no visible element has that role and name. The
// driver judges it only after the screen's present expectations settled, never on a blank page.
export function judgeExpectations(expect, observed) {
  return expect.map(({role,name,absent}) => {
    const exact=observed.find(item=>item.role===role && item.name===name);
    if (absent === true) {
      const gone=!exact || exact.count===0 || exact.visible!==true;
      return {role,name,absent:true,seen:gone,closest:null};
    }
    const seen=exact?.count===1 && exact.visible===true;
    const choices=observed.filter(item=>item.role===role && item.visible);
    choices.sort((a,b)=>distance(name,a.name)-distance(name,b.name) || (a.name<b.name?-1:a.name>b.name?1:0));
    return {role,name,seen,closest:seen || !choices.length ? null : {role,name:choices[0].name}};
  });
}
