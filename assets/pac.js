function isPlainHostName(host){return host.indexOf('.')===-1;}
function dnsDomainIs(host,domain){return host.endsWith(domain);}
function localHostOrDomainIs(host,hostdom){return host===hostdom||(!host.includes('.')&&hostdom.startsWith(host+'.'));}
function dnsDomainLevels(host){return host.split('.').length-1;}
function shExpMatch(str,pattern){return new RegExp('^'+pattern.replace(/[.+^${}()|[\]\\]/g,'\\$&').replace(/\*/g,'.*').replace(/\?/g,'.')+'$').test(str);}
function isResolvable(host){return dnsResolve(host)!==null;}
function isInNet(host,pattern,mask){var ip=dnsResolve(host);if(!ip)return false;function n(s){return s.split('.').reduce((a,b)=>(a<<8)|Number(b),0);}return (n(ip)&n(mask))===(n(pattern)&n(mask));}
function weekdayRange(first,last,gmt){var a=[...arguments],utc=a[a.length-1]==='GMT';if(utc)a.pop();var days=['SUN','MON','TUE','WED','THU','FRI','SAT'],d=new Date(),v=utc?d.getUTCDay():d.getDay(),s=days.indexOf(a[0]),e=a.length>1?days.indexOf(a[1]):s;return s<=e?v>=s&&v<=e:v>=s||v<=e;}
function timeRange(){var a=[...arguments],utc=a[a.length-1]==='GMT';if(utc)a.pop();var d=new Date(),h=utc?d.getUTCHours():d.getHours(),m=utc?d.getUTCMinutes():d.getMinutes(),s=utc?d.getUTCSeconds():d.getSeconds(),now=h*3600+m*60+s,start,end;if(a.length===1)return h===a[0];if(a.length===2){start=a[0]*3600;end=a[1]*3600+3599;}else if(a.length===4){start=a[0]*3600+a[1]*60;end=a[2]*3600+a[3]*60+59;}else{start=a[0]*3600+a[1]*60+a[2];end=a[3]*3600+a[4]*60+a[5];}return start<=end?now>=start&&now<=end:now>=start||now<=end;}
function dateRange(){
  var a=Array.from(arguments),utc=a[a.length-1]==='GMT';if(utc)a.pop();
  var d=new Date(),day=utc?d.getUTCDate():d.getDate(),month=utc?d.getUTCMonth():d.getMonth(),year=utc?d.getUTCFullYear():d.getFullYear();
  var months=['JAN','FEB','MAR','APR','MAY','JUN','JUL','AUG','SEP','OCT','NOV','DEC'];
  function between(n,a,b){return a<=b?n>=a&&n<=b:n>=a||n<=b;}
  if(a.length===1||a.length===2){var b=a.length===1?a[0]:a[1];return typeof a[0]==='string'?between(month,months.indexOf(a[0]),months.indexOf(b)):between(a[0]>31?year:day,a[0],b);}
  if(a.length===4){if(typeof a[0]==='string')return between(year*12+month,a[1]*12+months.indexOf(a[0]),a[3]*12+months.indexOf(a[2]));return between(month*32+day,months.indexOf(a[1])*32+a[0],months.indexOf(a[3])*32+a[2]);}
  if(a.length===6)return between(year*512+month*32+day,a[2]*512+months.indexOf(a[1])*32+a[0],a[5]*512+months.indexOf(a[4])*32+a[3]);return false;
}
function alert(message){}
