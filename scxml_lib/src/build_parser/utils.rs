pub fn controll_expression(s:String)->String{
    let c;
    if s.contains("&amp;&amp;"){
        c= s.replace("&amp;&amp;", "&&");
    }else {
        c = s;   
    }
    c
}