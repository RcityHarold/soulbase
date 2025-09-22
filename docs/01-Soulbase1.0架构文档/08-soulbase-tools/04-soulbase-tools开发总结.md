# SB-08 �� sb-tools �����ܽ�
## 1. ��ǰ������
- Manifest �㣺ʵ�� `ToolManifest` SemVer �汾��Consent scope ��ʾ��CompatMatrix������/Scope ���뼰 JSON-Schema ����У�飬ȷ������Ĭ�Ͼܾ�����СȨ�ޡ�
- Registry �㣺�� `InMemoryRegistry` �м�¼ policy_hash������ָ�ơ�LLM �ɼ��ԣ����� `update_policy` / `update_config_fingerprint` / `visible_only` ���ˣ�֧���ȸ����������׼�롣
- Preflight ���ţ�`PreflightService` ���� Schema У�顢Idempotency-Key ǿ�ơ�Consent/��Ȩ/����ж���ConfigSnapshot ָ��͸���������� Planned Ops + ProfileHash + Ԥ����ա�
- Invocation ���ţ�`InvokerImpl` ���������ݵȡ����� Planned Ops���ۺ� Sandbox Ԥ��/�����á�ִ�� obligations������ output/args ժҪ���κν�������� `ToolInvokeBegin/End` �¼���ָ���¼��
- �۲����¼����ṩ `ToolEventSink`��`ToolMetrics` Ĭ��ʵ�֣������ϲ���� sb-observe ���Զ����أ�������ʾ���� `crates/sb-tools/tests/basic.rs` ��֤��ע���Ԥ���ִ�С��ݵ����С���

## 2. ������ѡ��ǿ
1. ������ʵ Auth/QoS/Config Loader��ʵ������������������Ԥ��ؿۡ�
2. ��չ `ToolMetrics` / `ToolEventSink` �� sb-observe��EvidenceSink �����������Ǹ����ǩ��ָ�ꡣ
3. �ḻ obligations ���ԣ�������ˮӡ���ֶ�ӳ�䣩������ contract-testkit �в�����Լ������

## 3. �ο��ļ�
- ����ʵ�֣�`crates/sb-tools/src`
- ʾ�����ԣ�`crates/sb-tools/tests/basic.rs`
