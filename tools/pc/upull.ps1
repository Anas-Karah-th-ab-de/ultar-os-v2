param([string]$Dev,[string]$Local,[string]$Port="COM13")
function SendCmd($sp,$cmd,$tmo){
  $sp.DiscardInBuffer(); $sp.Write($cmd + "`n")
  $o=New-Object Text.StringBuilder; $dl=(Get-Date).AddMilliseconds($tmo)
  while((Get-Date)-lt $dl){ try{ $c=$sp.ReadExisting(); if($c){ [void]$o.Append($c); if($o.ToString()-match 'RC=-?\d+'){break} } else { Start-Sleep -Milliseconds 20 } }catch{} }
  $t=$o.ToString() -replace "`r",""
  $stx=[char]2; if($t.Contains($stx)){ $t=$t.Substring($t.IndexOf($stx)+1) }
  return $t
}
$sp=New-Object System.IO.Ports.SerialPort $Port,115200,None,8,One
$sp.ReadTimeout=500;$sp.WriteTimeout=8000;$sp.Open();Start-Sleep -Milliseconds 300
# 'bereit' abwarten
$sb=New-Object Text.StringBuilder;$dl=(Get-Date).AddMilliseconds(8000)
while((Get-Date)-lt $dl){try{$c=$sp.ReadExisting();if($c){[void]$sb.Append($c);if($sb.ToString()-match 'bereit'){break}}else{Start-Sleep -Milliseconds 60}}catch{}}
# Groesse holen
$szr = SendCmd $sp "wc -c < $Dev" 8000
$size = 0
foreach($line in ($szr -split "`n")){ if($line.Trim() -match '^\d+$'){ $size=[int]$line.Trim(); break } }
Write-Output "SIZE=$size"
$bs=2046
$fs=New-Object IO.StreamWriter($Local + ".b64", $false)
$blk=0
for($off=0; $off -lt $size; $off+=$bs){
  $r = SendCmd $sp "dd if=$Dev bs=$bs skip=$blk count=1 2>/dev/null | base64" 8000
  # nur base64-Zeichen behalten (Framing/RC/ready raus, auch die 'RC=0'-Zeile)
  $r = $r -replace 'RC=-?\d+',''
  $clean = ($r.ToCharArray() | Where-Object { ($_ -ge 'A' -and $_ -le 'Z') -or ($_ -ge 'a' -and $_ -le 'z') -or ($_ -ge '0' -and $_ -le '9') -or $_ -eq '+' -or $_ -eq '/' -or $_ -eq '=' }) -join ''
  $fs.Write($clean)
  $blk++
}
$fs.Close()
$sp.Close()
Write-Output "BLOCKS=$blk DONE"
